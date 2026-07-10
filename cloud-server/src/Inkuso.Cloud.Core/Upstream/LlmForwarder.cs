using System.Text;
using System.Text.Json;
using Microsoft.Extensions.Http;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Core.Upstream;

public class LlmForwarder
{
    private readonly IHttpClientFactory _httpFactory;
    private readonly AppDbContext _db;

    public LlmForwarder(IHttpClientFactory httpFactory, AppDbContext db)
    {
        _httpFactory = httpFactory;
        _db = db;
    }

    public record ChatRequestBody(
        string? Model,
        List<object>? Messages,
        bool? Stream,
        float? Temperature,
        int? MaxTokens
    );

    public record TokenUsage(long PromptTokens, long CompletionTokens, decimal CostCents);

    public record ForwardStreamResult(Stream UpstreamStream, TokenUsage? Usage);

    public async Task<ForwardStreamResult> ForwardStreamAsync(
        Guid userId,
        Guid modelConfigId,
        string requestBody,
        CancellationToken ct)
    {
        var modelConfig = await _db.ModelConfigs.FindAsync(new object[] { modelConfigId }, ct)
            ?? throw new InvalidOperationException("Model not found");

        var client = _httpFactory.CreateClient("upstream");
        client.DefaultRequestHeaders.Clear();
        client.DefaultRequestHeaders.Add("Authorization", $"Bearer {modelConfig.UpstreamApiKey}");
        client.Timeout = TimeSpan.FromSeconds(120);

        var upstreamUrl = $"{modelConfig.UpstreamBaseUrl.TrimEnd('/')}/chat/completions";
        var upstreamReq = new HttpRequestMessage(HttpMethod.Post, upstreamUrl)
        {
            Content = new StringContent(requestBody, Encoding.UTF8, "application/json"),
        };

        var response = await client.SendAsync(upstreamReq, HttpCompletionOption.ResponseHeadersRead, ct);

        if (!response.IsSuccessStatusCode)
        {
            var errBody = await response.Content.ReadAsStringAsync(ct);
            throw new HttpRequestException($"Upstream returned {(int)response.StatusCode}: {errBody}");
        }

        var stream = await response.Content.ReadAsStreamAsync(ct);
        return new ForwardStreamResult(stream, null);
    }

    public static decimal CalculateCost(ModelConfig config, long promptTokens, long completionTokens, long cachedPromptTokens = 0)
    {
        // Cached tokens are a subset of prompt tokens (typically OpenAI/Anthropic-style prompt caching).
        // Bill cached portion at the (cheaper) cached-input price, the rest at normal input price.
        var cached = Math.Min(Math.Max(cachedPromptTokens, 0), promptTokens);
        var uncachedPrompt = promptTokens - cached;

        var cachedCost = (decimal)cached / 1_000_000m * config.CachedInputPricePerMTokens;
        var inputCost = (decimal)uncachedPrompt / 1_000_000m * config.InputPricePerMTokens;
        var outputCost = (decimal)completionTokens / 1_000_000m * config.OutputPricePerMTokens;
        return Math.Round((cachedCost + inputCost + outputCost) * 100, 2); // return in cents
    }

    public async Task RecordUsageAsync(Guid userId, Guid modelConfigId, long promptTokens, long completionTokens, long cachedPromptTokens, CancellationToken ct)
    {
        var config = await _db.ModelConfigs.FindAsync(new object[] { modelConfigId }, ct);
        if (config == null) return;

        var costCents = CalculateCost(config, promptTokens, completionTokens, cachedPromptTokens);
        if (costCents <= 0) return;

        var record = new UsageRecord
        {
            UserId = userId,
            ModelConfigId = modelConfigId,
            PromptTokens = promptTokens,
            CachedPromptTokens = cachedPromptTokens,
            CompletionTokens = completionTokens,
            CostCents = costCents,
        };

        _db.UsageRecords.Add(record);
        await _db.SaveChangesAsync(ct);

        // Deduct from user balance using optimistic locking
        var rows = await _db.Users
            .Where(u => u.Id == userId && u.BalanceCents >= costCents)
            .ExecuteUpdateAsync(s => s.SetProperty(u => u.BalanceCents, u => u.BalanceCents - costCents), ct);

        if (rows == 0)
        {
            throw new InvalidOperationException("Insufficient balance");
        }
    }
}
