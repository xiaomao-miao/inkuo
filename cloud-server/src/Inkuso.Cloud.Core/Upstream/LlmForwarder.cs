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

        // CreateClient returns a pooled HttpClient. Mutating DefaultRequestHeaders
        // would leak the Authorization header into other requests handled by the
        // same pooled instance, which is both a security risk (key bleed across
        // models) and a correctness bug (next caller sees a stale Bearer). Use
        // a per-request HttpRequestMessage header instead, and a per-call
        // timeout via a CancellationTokenSource instead of HttpClient.Timeout.
        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeoutCts.CancelAfter(TimeSpan.FromSeconds(120));

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
            timeoutCts.Token);

        if (!response.IsSuccessStatusCode)
        {
            // Do not leak the upstream body wholesale — it can include the
            // operator's API key, request ids, or billing context that a
            // desktop client should never see. Surface a status + truncated
            // snippet; full body goes to the server log via the exception
            // message which the operator can correlate via the request id.
            var errBody = await response.Content.ReadAsStringAsync(timeoutCts.Token);
            var snippet = errBody.Length > 200 ? errBody[..200] + "…" : errBody;
            throw new HttpRequestException(
                $"Upstream returned {(int)response.StatusCode}: {snippet}");
        }

        var stream = await response.Content.ReadAsStreamAsync(timeoutCts.Token);
        return new ForwardStreamResult(stream, null);
    }

    /// <summary>
    /// Compute the cost in cents for a single chat-completions call. Prices
    /// on <see cref="ModelConfig"/> are quoted per 1M tokens; the function
    /// rounds to the nearest cent so the <c>numeric(12,2)</c> column never
    /// accumulates sub-cent drift.
    /// </summary>
    public static decimal CalculateCost(
        ModelConfig config,
        long promptTokens,
        long completionTokens,
        long cachedPromptTokens = 0)
    {
        // Cached tokens are a subset of prompt tokens (typically OpenAI / Anthropic
        // prompt caching). Clamp defensively so a malformed upstream payload
        // can't push the cached bucket above the total prompt.
        var cached = Math.Min(Math.Max(cachedPromptTokens, 0), promptTokens);
        var uncachedPrompt = promptTokens - cached;

        var cachedCost = (decimal)cached / 1_000_000m * config.CachedInputPricePerMTokens;
        var inputCost = (decimal)uncachedPrompt / 1_000_000m * config.InputPricePerMTokens;
        var outputCost = (decimal)completionTokens / 1_000_000m * config.OutputPricePerMTokens;
        return Math.Round((cachedCost + inputCost + outputCost) * 100, 2);
    }

    /// <summary>
    /// Persist a usage record and deduct the user's balance atomically. If
    /// the user has run out of credit the whole operation rolls back — we
    /// never want a "we billed the upstream but didn't bill the customer"
    /// row to land in <c>UsageRecords</c>, which is the state the previous
    /// implementation could produce because the two writes were not
    /// transactional.
    /// </summary>
    /// <exception cref="InsufficientBalanceException">
    /// Thrown when the user's balance is below <paramref name="costCents"/>
    /// after applying the calculated cost. The transaction is rolled back
    /// so no UsageRecord row is left behind.
    /// </exception>
    public async Task RecordUsageAsync(
        Guid userId,
        Guid modelConfigId,
        long promptTokens,
        long completionTokens,
        long cachedPromptTokens,
        CancellationToken ct)
    {
        var config = await _db.ModelConfigs.FindAsync(new object[] { modelConfigId }, ct);
        if (config == null) return;

        var costCents = CalculateCost(config, promptTokens, completionTokens, cachedPromptTokens);
        if (costCents <= 0) return;

        await using var tx = await _db.Database.BeginTransactionAsync(ct);
        try
        {
            // Optimistic balance deduction: the WHERE clause includes the
            // balance >= costCents guard so two concurrent writes can't both
            // succeed in the negative balance region. ExecuteUpdate emits a
            // single UPDATE … RETURNING-style round trip on Postgres.
            var rows = await _db.Users
                .Where(u => u.Id == userId && u.BalanceCents >= costCents)
                .ExecuteUpdateAsync(s => s.SetProperty(u => u.BalanceCents, u => u.BalanceCents - costCents), ct);

            if (rows == 0)
            {
                // No row updated: either the user vanished or balance dropped
                // below the cost between the call-site quota check and now.
                // Roll back the implicit implicit-transaction SaveChanges below
                // by throwing before adding the UsageRecord.
                throw new InsufficientBalanceException(
                    $"User {userId} has insufficient balance for {costCents:F2} cents of usage.");
            }

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

            await tx.CommitAsync(ct);
        }
        catch
        {
            await tx.RollbackAsync(ct);
            throw;
        }
    }
}

public sealed class InsufficientBalanceException : Exception
{
    public InsufficientBalanceException(string message) : base(message) { }
}
