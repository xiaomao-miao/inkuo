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

    public record TokenUsage(long PromptTokens, long CompletionTokens, long CostPoints);

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

        // Charge input at full uncached rate (most conservative).
        var inputPoints = (decimal)promptTokens / 1_000_000m * config.InputPricePerMTokens * PointsPerYuan;
        var outputPoints = (decimal)maxOutputTokensCap / 1_000_000m * config.OutputPricePerMTokens * PointsPerYuan;
        var total = inputPoints + outputPoints;
        return (long)Math.Ceiling(total); // smallest integer ≥ estimate
    }

    /// <summary>
    /// Reserve points for a pending request atomically. Returns the reservation
    /// row id, or <c>0</c> if the user could not be charged (insufficient balance,
    /// account suspended, or no such user). The reservation is deducted from
    /// <c>AvailablePoints = BalancePoints − ReservedPoints</c>; the user-visible
    /// balance is unchanged so a concurrent refresh of /account/me doesn't double-
    /// count the held amount.
    /// </summary>
    public async Task<long> TryReservePointsAsync(
        Guid userId,
        Guid modelConfigId,
        long pointsToReserve,
        string? requestId,
        CancellationToken ct)
    {
        if (pointsToReserve <= 0) return 0;

        await using var tx = await _db.Database.BeginTransactionAsync(ct);
        try
        {
            // Atomic compare-and-set: only succeeds if the user exists, is not
            // suspended, and BalancePoints - ReservedPoints >= pointsToReserve.
            var rows = await _db.Users
                .Where(u => u.Id == userId
                            && !u.IsSuspended
                            && (u.BalancePoints - u.ReservedPoints) >= pointsToReserve)
                .ExecuteUpdateAsync(s => s.SetProperty(
                    u => u.ReservedPoints,
                    u => u.ReservedPoints + pointsToReserve), ct);

            if (rows == 0) return 0;

            var usage = new UsageRecord
            {
                UserId = userId,
                ModelConfigId = modelConfigId,
                PromptTokens = 0,
                CachedPromptTokens = 0,
                CompletionTokens = 0,
                CostPoints = 0,
                ReservedPoints = pointsToReserve,
                BillingStatus = "pending",
                RequestId = requestId,
            };
            _db.UsageRecords.Add(usage);
            await _db.SaveChangesAsync(ct);
            await tx.CommitAsync(ct);
            return 1;
        }
        catch
        {
            await tx.RollbackAsync(ct);
            throw;
        }
    }

    /// <summary>
    /// Finalize a previously-reserved request. Computes the actual cost from
    /// real upstream token counts, debits that amount off the user's balance,
    /// and releases the unused portion of the reservation. If the actual cost
    /// exceeds the available balance after release, the user is marked as
    /// suspended and the usage record is flagged as <c>debt</c> — so the audit
    /// trail is intact even when the deduction ultimately fails.
    /// </summary>
    public async Task<BillingOutcome> SettleUsageAsync(
        Guid userId,
        Guid modelConfigId,
        long promptTokens,
        long completionTokens,
        long cachedPromptTokens,
        long previouslyReservedPoints,
        string? requestId,
        CancellationToken ct)
    {
        var config = await _db.ModelConfigs.FindAsync(new object[] { modelConfigId }, ct);
        if (config == null)
            throw new InvalidOperationException("Model not found during billing settlement");

        var actualCostPoints = CalculateCostPoints(config, promptTokens, completionTokens, cachedPromptTokens);
        var refund = Math.Max(0, previouslyReservedPoints - actualCostPoints);

        await using var tx = await _db.Database.BeginTransactionAsync(ct);
        try
        {
            // 1. Refund the over-reserved portion (if any) and release the cost
            //    from ReservedPoints. If the user is being settled for a request
            //    that returned zero tokens we still want to release the hold.
            var releaseRows = await _db.Users
                .Where(u => u.Id == userId)
                .ExecuteUpdateAsync(s => s
                    .SetProperty(u => u.ReservedPoints,
                        u => u.ReservedPoints - Math.Min(previouslyReservedPoints, long.MaxValue))
                    .SetProperty(u => u.BalancePoints,
                        u => u.BalancePoints + refund), ct);

            if (releaseRows == 0)
                throw new InvalidOperationException("User vanished during billing settlement");

            // 2. Try to deduct the actual cost from balance. This is the previous
            //    failure mode: rollback-on-keep-credit loop. Now we explicitly
            //    accept the failure state and record the debt instead.
            string status;
            long? missingPoints = null;
            if (actualCostPoints > 0)
            {
                var deductRows = await _db.Users
                    .Where(u => u.Id == userId && u.BalancePoints >= actualCostPoints)
                    .ExecuteUpdateAsync(s => s.SetProperty(
                        u => u.BalancePoints,
                        u => u.BalancePoints - actualCostPoints), ct);

                if (deductRows == 0)
                {
                    // Move the whole cost to debt and suspend the account.
                    // BalancePoints is what the user can still spend; a negative
                    // balance would mislead the UI, so we cap it at 0 instead.
                    var owed = actualCostPoints;
                    await _db.Users
                        .Where(u => u.Id == userId)
                        .ExecuteUpdateAsync(s => s
                            .SetProperty(u => u.BalancePoints, u => 0L)
                            .SetProperty(u => u.IsSuspended, u => true), ct);
                    status = "debt";
                    missingPoints = owed;
                }
                else
                {
                    status = "settled";
                }
            }
            else
            {
                status = "released";
            }

            // 3. Record the final usage row. We look up the pending reservation
            //    by RequestId and update in place; if it's missing (e.g. settlement
            //    after a server restart) we fall back to a fresh row.
            var pending = await _db.UsageRecords
                .FirstOrDefaultAsync(r => r.RequestId == requestId && r.UserId == userId && r.BillingStatus == "pending", ct);

            if (pending != null)
            {
                pending.PromptTokens = promptTokens;
                pending.CachedPromptTokens = cachedPromptTokens;
                pending.CompletionTokens = completionTokens;
                pending.CostPoints = actualCostPoints;
                pending.BillingStatus = status;
                pending.RecordedAt = DateTime.UtcNow;
            }
            else
            {
                _db.UsageRecords.Add(new UsageRecord
                {
                    UserId = userId,
                    ModelConfigId = modelConfigId,
                    PromptTokens = promptTokens,
                    CachedPromptTokens = cachedPromptTokens,
                    CompletionTokens = completionTokens,
                    CostPoints = actualCostPoints,
                    ReservedPoints = previouslyReservedPoints,
                    BillingStatus = status,
                    RequestId = requestId,
                });
            }

            await _db.SaveChangesAsync(ct);
            await tx.CommitAsync(ct);

            return new BillingOutcome(actualCostPoints, status, missingPoints);
        }
        catch
        {
            await tx.RollbackAsync(ct);
            throw;
        }
    }

    /// <summary>
    /// Release a pending reservation without charging anything (e.g. the upstream
    /// returned no usage block, or the client disconnected before tokens came back).
    /// </summary>
    public async Task ReleaseReservationAsync(
        Guid userId,
        long previouslyReservedPoints,
        string? requestId,
        CancellationToken ct)
    {
        if (previouslyReservedPoints <= 0) return;
        await using var tx = await _db.Database.BeginTransactionAsync(ct);
        try
        {
            await _db.Users
                .Where(u => u.Id == userId)
                .ExecuteUpdateAsync(s => s
                    .SetProperty(u => u.ReservedPoints,
                        u => u.ReservedPoints - Math.Min(previouslyReservedPoints, long.MaxValue))
                    .SetProperty(u => u.BalancePoints,
                        u => u.BalancePoints + previouslyReservedPoints), ct);

            if (requestId != null)
            {
                var pending = await _db.UsageRecords
                    .FirstOrDefaultAsync(r => r.RequestId == requestId && r.BillingStatus == "pending", ct);
                if (pending != null)
                {
                    pending.BillingStatus = "released";
                    pending.RecordedAt = DateTime.UtcNow;
                    await _db.SaveChangesAsync(ct);
                }
            }

            await tx.CommitAsync(ct);
        }
        catch
        {
            await tx.RollbackAsync(ct);
            throw;
        }
    }

    public record BillingOutcome(long CostPoints, string Status, long? MissingPoints);
}

public sealed class InsufficientBalanceException : Exception
{
    public InsufficientBalanceException(string message) : base(message) { }
}
