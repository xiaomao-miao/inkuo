using System.Text;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Security;

namespace Inkuso.Cloud.Core.Upstream;

public class LlmForwarder
{
    private readonly IHttpClientFactory _httpFactory;
    private readonly AppDbContext _db;
    private readonly ISecretProtector _secrets;

    public LlmForwarder(IHttpClientFactory httpFactory, AppDbContext db, ISecretProtector secrets)
    {
        _httpFactory = httpFactory;
        _db = db;
        _secrets = secrets;
    }

    public record ChatRequestBody(
        string? Model,
        List<object>? Messages,
        bool? Stream,
        float? Temperature,
        int? MaxTokens);

    public static readonly TimeSpan MaxStreamDuration = TimeSpan.FromMinutes(5);

    public sealed class ForwardStreamResult(
        HttpResponseMessage response,
        Stream upstreamStream) : IDisposable
    {
        public Stream UpstreamStream { get; } = upstreamStream;

        public void Dispose()
        {
            UpstreamStream.Dispose();
            response.Dispose();
        }
    }

    public async Task<ForwardStreamResult> ForwardStreamAsync(
        Guid userId,
        Guid modelConfigId,
        string requestBody,
        CancellationToken ct)
    {
        var modelConfig = await _db.ModelConfigs.FindAsync(new object[] { modelConfigId }, ct)
            ?? throw new InvalidOperationException("Model not found");

        // CreateClient returns a pooled HttpClient. Mutating DefaultRequestHeaders
        // would leak the Authorization header into other requests handled by the
        // same pooled instance, which is both a security risk (key bleed across
        // models) and a correctness bug (next caller sees a stale Bearer). Use
        // a per-request HttpRequestMessage header instead. The endpoint owns a
        // bounded token for the complete response body, so a client disconnect
        // can stop writes without aborting usage collection and billing.

        var client = _httpFactory.CreateClient("upstream");
        var upstreamUrl = $"{modelConfig.UpstreamBaseUrl.TrimEnd('/')}/chat/completions";
        using var upstreamReq = new HttpRequestMessage(HttpMethod.Post, upstreamUrl)
        {
            Content = new StringContent(requestBody, Encoding.UTF8, "application/json"),
        };
        upstreamReq.Headers.TryAddWithoutValidation(
            "Authorization",
            $"Bearer {_secrets.Unprotect(modelConfig.UpstreamApiKey)}");

        var response = await client.SendAsync(
            upstreamReq,
            HttpCompletionOption.ResponseHeadersRead,
            ct);

        if (!response.IsSuccessStatusCode)
        {
            // Never include upstream response bodies in exceptions: providers
            // may echo credentials, prompts, or account metadata. The caller
            // logs the local request id for operator correlation.
            var status = (int)response.StatusCode;
            response.Dispose();
            throw new HttpRequestException(
                $"Upstream returned HTTP {status}.");
        }

        try
        {
            var stream = await response.Content.ReadAsStreamAsync(ct);
            return new ForwardStreamResult(response, stream);
        }
        catch
        {
            response.Dispose();
            throw;
        }
    }

    // --- public constants / structs -------------------------------------------------

    /// <summary>Conversion rate: 1 元 = 1000 点. All point arithmetic uses this constant.</summary>
    public const long PointsPerYuan = 1000;

    /// <summary>
    /// Cost of a single chat-completions call in <b>points</b> (1 元 = 1000 点).
    /// ModelConfig prices are stored as yuan per 1M tokens (admin-friendly unit),
    /// so the yuan value is multiplied by PointsPerYuan to obtain points.
    /// </summary>
    public static long CalculateCostPoints(
        ModelConfig config,
        long promptTokens,
        long completionTokens,
        long cachedPromptTokens = 0)
    {
        if (promptTokens < 0 || completionTokens < 0 || cachedPromptTokens < 0) return 0;
        ValidateRateCard(config);

        // Cached tokens are a subset of prompt tokens. Clamp defensively so a
        // malformed upstream payload can't push the cached bucket above the total.
        var cached = Math.Min(cachedPromptTokens, promptTokens);
        var uncachedPrompt = promptTokens - cached;

        // Yuan amounts in points (1 yuan = PointsPerYuan points, scale per-million).
        var cachedCostPoints = (decimal)cached / 1_000_000m * config.CachedInputPricePerMTokens * PointsPerYuan;
        var inputCostPoints = (decimal)uncachedPrompt / 1_000_000m * config.InputPricePerMTokens * PointsPerYuan;
        var outputCostPoints = (decimal)completionTokens / 1_000_000m * config.OutputPricePerMTokens * PointsPerYuan;

        // Bills use points as the smallest unit (1 元 = 1000 点). Any non-zero
        // consumption must charge at least 1 point — otherwise sub-half-yuan
        // usage would round to zero cost and the user would effectively run
        // tokens for free. Ceiling rounds 0.1 点 up to 1 点 (= 0.001 元);
        // 0.5 点 rounds up to 1 点 rather than AwayFromZero's "0.5 → 1".
        // Net effect: every billable token request costs ≥ 1 point = 0.001 元.
        var total = cachedCostPoints + inputCostPoints + outputCostPoints;
        if (total <= 0) return 0;
        return (long)Math.Ceiling(total);
    }

    /// <summary>
    /// Conservative upper bound for the cost (in points) of a chat call, used to
    /// pre-authorize a reservation before contacting the upstream. The estimate
    /// assumes the worst case for the unknown output side: max_tokens (or a
    /// configured default) at full output price, plus an input budget derived
    /// from the prompt length.
    /// </summary>
    public static long EstimateMaxCostPoints(
        ModelConfig config,
        int promptTokens,
        int maxOutputTokensCap)
    {
        if (promptTokens < 0) promptTokens = 0;
        if (maxOutputTokensCap < 0) maxOutputTokensCap = 0;
        ValidateRateCard(config);

        // Charge input at full uncached rate (most conservative).
        var maximumInputPrice = Math.Max(
            config.InputPricePerMTokens,
            config.CachedInputPricePerMTokens);
        var inputPoints = (decimal)promptTokens / 1_000_000m * maximumInputPrice * PointsPerYuan;
        var outputPoints = (decimal)maxOutputTokensCap / 1_000_000m * config.OutputPricePerMTokens * PointsPerYuan;
        var total = inputPoints + outputPoints;
        return (long)Math.Ceiling(total); // smallest integer ≥ estimate
    }

    private static void ValidateRateCard(ModelConfig config)
    {
        if (config.InputPricePerMTokens < 0
            || config.OutputPricePerMTokens < 0
            || config.CachedInputPricePerMTokens < 0)
        {
            throw new InvalidOperationException("Model token prices cannot be negative.");
        }
    }

}
