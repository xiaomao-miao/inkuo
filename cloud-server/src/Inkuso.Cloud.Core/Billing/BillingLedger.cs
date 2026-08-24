using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Upstream;

namespace Inkuso.Cloud.Core.Billing;

/// <summary>
/// Owns the point-ledger state machine. BalancePoints is the user's total
/// unspent credit; ReservedPoints is the frozen subset of that balance.
/// Reserving therefore never changes BalancePoints, and releasing a hold
/// must never credit BalancePoints.
/// </summary>
public sealed class BillingLedger(AppDbContext db)
{
    public enum ReservationState
    {
        Reserved,
        AlreadyPending,
        AlreadyCompleted,
        Rejected,
    }

    public sealed record ReservationResult(
        ReservationState State,
        Guid? UsageRecordId,
        long ReservedPoints,
        string? BillingStatus)
    {
        public bool CanForward => State == ReservationState.Reserved;
    }

    public sealed record SettlementResult(
        long CostPoints,
        long ChargedPoints,
        long DebtPoints,
        string Status,
        bool Applied);

    public sealed record PricingSnapshot(
        decimal InputPricePerMTokens,
        decimal OutputPricePerMTokens,
        decimal CachedInputPricePerMTokens);

    public async Task<ReservationResult> TryReserveAsync(
        Guid userId,
        Guid modelConfigId,
        long pointsToReserve,
        string requestId,
        CancellationToken ct,
        PricingSnapshot? pricingSnapshot = null)
    {
        if (string.IsNullOrWhiteSpace(requestId))
            throw new ArgumentException("A request id is required for billing idempotency.", nameof(requestId));
        if (pointsToReserve < 0)
            throw new ArgumentOutOfRangeException(nameof(pointsToReserve));

        var pricing = pricingSnapshot;
        if (pricing is null)
        {
            pricing = await db.ModelConfigs.AsNoTracking()
                .Where(model => model.Id == modelConfigId)
                .Select(model => new PricingSnapshot(
                    model.InputPricePerMTokens,
                    model.OutputPricePerMTokens,
                    model.CachedInputPricePerMTokens))
                .SingleOrDefaultAsync(ct)
                ?? throw new BillingInvariantException(
                    "Cannot reserve against a missing model configuration.");
        }

        var existing = await db.UsageRecords.AsNoTracking()
            .SingleOrDefaultAsync(r => r.UserId == userId && r.RequestId == requestId, ct);
        if (existing != null)
            return ExistingReservation(existing);

        await using var tx = await db.Database.BeginTransactionAsync(ct);
        try
        {
            var rows = await db.Users
                .Where(u => u.Id == userId
                            && !u.IsSuspended
                            && u.BalancePoints >= 0
                            && u.ReservedPoints >= 0
                            && u.ReservedPoints <= u.BalancePoints
                            && (u.BalancePoints - u.ReservedPoints) >= pointsToReserve)
                .ExecuteUpdateAsync(s => s.SetProperty(
                    u => u.ReservedPoints,
                    u => u.ReservedPoints + pointsToReserve), ct);

            if (rows != 1)
            {
                await tx.RollbackAsync(ct);
                return new ReservationResult(ReservationState.Rejected, null, 0, null);
            }

            var usage = new UsageRecord
            {
                UserId = userId,
                ModelConfigId = modelConfigId,
                PromptTokens = 0,
                CachedPromptTokens = 0,
                CompletionTokens = 0,
                CostPoints = 0,
                ReservedPoints = pointsToReserve,
                InputPricePerMTokensSnapshot = pricing.InputPricePerMTokens,
                OutputPricePerMTokensSnapshot = pricing.OutputPricePerMTokens,
                CachedInputPricePerMTokensSnapshot = pricing.CachedInputPricePerMTokens,
                BillingStatus = "pending",
                RequestId = requestId,
            };
            db.UsageRecords.Add(usage);
            await db.SaveChangesAsync(ct);
            await tx.CommitAsync(ct);
            return new ReservationResult(
                ReservationState.Reserved,
                usage.Id,
                pointsToReserve,
                usage.BillingStatus);
        }
        catch (DbUpdateException)
        {
            await tx.RollbackAsync(CancellationToken.None);
            // A concurrent retry can pass the optimistic pre-check and then
            // lose the unique (UserId, RequestId) insert race. Detach the
            // failed Added entity and resolve the winner as an idempotent
            // duplicate instead of surfacing a 500 or forwarding twice.
            db.ChangeTracker.Clear();
            var winner = await db.UsageRecords.AsNoTracking()
                .SingleOrDefaultAsync(
                    r => r.UserId == userId && r.RequestId == requestId,
                    ct);
            if (winner != null)
                return ExistingReservation(winner);
            throw;
        }
        catch
        {
            await tx.RollbackAsync(CancellationToken.None);
            throw;
        }
    }

    /// <summary>
    /// Marks the point at which an upstream accepted the request. A streaming
    /// hold is never refunded by the generic pending-request cleanup: either
    /// its usage is queued for exact settlement or stale reconciliation bills
    /// the held maximum after the bounded stream lifetime has elapsed.
    /// </summary>
    public async Task<bool> MarkStreamingAsync(
        Guid userId,
        string requestId,
        CancellationToken ct)
    {
        var rows = await db.UsageRecords
            .Where(record => record.UserId == userId
                             && record.RequestId == requestId
                             && record.BillingStatus == "pending")
            .ExecuteUpdateAsync(update => update
                .SetProperty(record => record.BillingStatus, "streaming")
                .SetProperty(record => record.RecordedAt, DateTime.UtcNow), ct);
        if (rows == 1) return true;

        var status = await db.UsageRecords.AsNoTracking()
            .Where(record => record.UserId == userId && record.RequestId == requestId)
            .Select(record => record.BillingStatus)
            .SingleOrDefaultAsync(ct);
        if (status is "streaming" or "bill_pending" or "settled" or "released" or "debt" or "estimated")
            return false;
        throw new BillingInvariantException($"Reservation {requestId} cannot enter streaming state.");
    }

    /// <summary>
    /// Durably records usage before attempting the money movement. If the
    /// process or database fails during settlement, the billing worker can
    /// replay this row without relying on in-memory token counters.
    /// </summary>
    public async Task<bool> QueueSettlementAsync(
        Guid userId,
        string requestId,
        long promptTokens,
        long completionTokens,
        long cachedPromptTokens,
        CancellationToken ct)
    {
        if (promptTokens < 0 || completionTokens < 0 || cachedPromptTokens < 0)
            throw new ArgumentOutOfRangeException(nameof(promptTokens));

        var rows = await db.UsageRecords
            .Where(record => record.UserId == userId
                             && record.RequestId == requestId
                             && record.BillingStatus == "streaming")
            .ExecuteUpdateAsync(update => update
                .SetProperty(record => record.PromptTokens, promptTokens)
                .SetProperty(record => record.CompletionTokens, completionTokens)
                .SetProperty(record => record.CachedPromptTokens, cachedPromptTokens)
                .SetProperty(record => record.BillingStatus, "bill_pending")
                .SetProperty(record => record.RecordedAt, DateTime.UtcNow), ct);
        if (rows == 1) return true;

        var status = await db.UsageRecords.AsNoTracking()
            .Where(record => record.UserId == userId && record.RequestId == requestId)
            .Select(record => record.BillingStatus)
            .SingleOrDefaultAsync(ct);
        if (status is "bill_pending" or "settled" or "released" or "debt" or "estimated")
            return false;
        throw new BillingInvariantException($"Reservation {requestId} cannot queue settlement.");
    }

    public async Task<SettlementResult> SettleAsync(
        Guid userId,
        Guid modelConfigId,
        long promptTokens,
        long completionTokens,
        long cachedPromptTokens,
        string requestId,
        CancellationToken ct)
    {
        await using var tx = await db.Database.BeginTransactionAsync(ct);
        try
        {
            var claimed = await db.UsageRecords
                .Where(r => r.UserId == userId
                            && r.ModelConfigId == modelConfigId
                            && r.RequestId == requestId
                            && (r.BillingStatus == "pending" || r.BillingStatus == "bill_pending"))
                .ExecuteUpdateAsync(s => s.SetProperty(r => r.BillingStatus, "settling"), ct);

            if (claimed != 1)
            {
                var terminal = await db.UsageRecords.AsNoTracking()
                    .SingleOrDefaultAsync(r => r.UserId == userId && r.RequestId == requestId, ct)
                    ?? throw new BillingInvariantException($"Reservation {requestId} does not exist.");
                await tx.RollbackAsync(ct);
                return ExistingSettlement(terminal);
            }

            var pending = await db.UsageRecords.AsNoTracking()
                .SingleAsync(r => r.UserId == userId
                                  && r.ModelConfigId == modelConfigId
                                  && r.RequestId == requestId
                                  && r.BillingStatus == "settling", ct);
            var config = await db.ModelConfigs.AsNoTracking()
                .SingleOrDefaultAsync(m => m.Id == modelConfigId, ct)
                ?? throw new BillingInvariantException("Model vanished during billing settlement.");

            var frozenPricing = new ModelConfig
            {
                InputPricePerMTokens = pending.InputPricePerMTokensSnapshot
                    ?? config.InputPricePerMTokens,
                OutputPricePerMTokens = pending.OutputPricePerMTokensSnapshot
                    ?? config.OutputPricePerMTokens,
                CachedInputPricePerMTokens = pending.CachedInputPricePerMTokensSnapshot
                    ?? config.CachedInputPricePerMTokens,
            };

            var held = pending.ReservedPoints ?? 0;
            var actual = LlmForwarder.CalculateCostPoints(
                frozenPricing, promptTokens, completionTokens, cachedPromptTokens);
            var charged = Math.Min(actual, held);
            var debt = Math.Max(0, actual - charged);
            var status = debt > 0 ? "debt" : actual > 0 ? "settled" : "released";

            var userRows = debt > 0
                ? await db.Users
                    .Where(u => u.Id == userId
                                && u.ReservedPoints >= held
                                && u.BalancePoints >= charged)
                    .ExecuteUpdateAsync(s => s
                        .SetProperty(u => u.BalancePoints, u => u.BalancePoints - charged)
                        .SetProperty(u => u.ReservedPoints, u => u.ReservedPoints - held)
                        .SetProperty(u => u.DebtPoints, u => u.DebtPoints + debt)
                        .SetProperty(u => u.IsSuspended, true), ct)
                : await db.Users
                    .Where(u => u.Id == userId
                                && u.ReservedPoints >= held
                                && u.BalancePoints >= charged)
                    .ExecuteUpdateAsync(s => s
                        .SetProperty(u => u.BalancePoints, u => u.BalancePoints - charged)
                        .SetProperty(u => u.ReservedPoints, u => u.ReservedPoints - held), ct);

            if (userRows != 1)
                throw new BillingInvariantException(
                    $"Cannot settle reservation {requestId}: user ledger invariant failed.");

            var usageRows = await db.UsageRecords
                .Where(r => r.Id == pending.Id && r.BillingStatus == "settling")
                .ExecuteUpdateAsync(s => s
                    .SetProperty(r => r.PromptTokens, promptTokens)
                    .SetProperty(r => r.CachedPromptTokens, cachedPromptTokens)
                    .SetProperty(r => r.CompletionTokens, completionTokens)
                    .SetProperty(r => r.CostPoints, actual)
                    .SetProperty(r => r.BillingStatus, status)
                    .SetProperty(r => r.RecordedAt, DateTime.UtcNow), ct);
            if (usageRows != 1)
                throw new BillingInvariantException($"Reservation {requestId} lost its settlement claim.");

            await tx.CommitAsync(ct);
            return new SettlementResult(actual, charged, debt, status, true);
        }
        catch
        {
            await tx.RollbackAsync(CancellationToken.None);
            throw;
        }
    }

    public async Task<int> RetryQueuedSettlementsAsync(int batchSize, CancellationToken ct)
    {
        var queued = await db.UsageRecords.AsNoTracking()
            .Where(record => record.BillingStatus == "bill_pending")
            .OrderBy(record => record.RecordedAt)
            .Select(record => new
            {
                record.UserId,
                record.ModelConfigId,
                record.RequestId,
                record.PromptTokens,
                record.CompletionTokens,
                record.CachedPromptTokens,
            })
            .Take(Math.Clamp(batchSize, 1, 500))
            .ToListAsync(ct);

        var settled = 0;
        List<Exception>? failures = null;
        foreach (var item in queued)
        {
            if (item.RequestId is null) continue;
            try
            {
                var outcome = await SettleAsync(
                    item.UserId,
                    item.ModelConfigId,
                    item.PromptTokens,
                    item.CompletionTokens,
                    item.CachedPromptTokens,
                    item.RequestId,
                    ct);
                if (outcome.Applied) settled++;
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                throw;
            }
            catch (Exception ex)
            {
                failures ??= [];
                failures.Add(ex);
            }
        }

        if (failures is { Count: > 0 })
            throw new AggregateException(
                $"Failed to retry {failures.Count} queued billing settlement(s).",
                failures);
        return settled;
    }

    /// <summary>
    /// Conservatively settles streams whose host disappeared after an upstream
    /// accepted the request but before a usage block could be persisted. The
    /// full hold is charged; the bounded upstream timeout guarantees a live
    /// request cannot reach this state while it is still running.
    /// </summary>
    public async Task<int> SettleStaleStreamsAsync(
        DateTime cutoff,
        int batchSize,
        CancellationToken ct)
    {
        var ids = await db.UsageRecords.AsNoTracking()
            .Where(record => record.BillingStatus == "streaming" && record.RecordedAt < cutoff)
            .OrderBy(record => record.RecordedAt)
            .Select(record => record.Id)
            .Take(Math.Clamp(batchSize, 1, 500))
            .ToListAsync(ct);

        var settled = 0;
        List<Exception>? failures = null;
        foreach (var id in ids)
        {
            await using var tx = await db.Database.BeginTransactionAsync(ct);
            try
            {
                var claimed = await db.UsageRecords
                    .Where(record => record.Id == id
                                     && record.BillingStatus == "streaming"
                                     && record.RecordedAt < cutoff)
                    .ExecuteUpdateAsync(update => update.SetProperty(
                        record => record.BillingStatus,
                        "settling"), ct);
                if (claimed != 1)
                {
                    await tx.RollbackAsync(ct);
                    continue;
                }

                var usage = await db.UsageRecords.AsNoTracking()
                    .SingleAsync(record => record.Id == id && record.BillingStatus == "settling", ct);
                var held = usage.ReservedPoints ?? 0;
                var userRows = await db.Users
                    .Where(user => user.Id == usage.UserId
                                   && user.ReservedPoints >= held
                                   && user.BalancePoints >= held)
                    .ExecuteUpdateAsync(update => update
                        .SetProperty(user => user.BalancePoints, user => user.BalancePoints - held)
                        .SetProperty(user => user.ReservedPoints, user => user.ReservedPoints - held), ct);
                if (userRows != 1)
                    throw new BillingInvariantException(
                        $"Cannot conservatively settle {usage.RequestId}: user ledger invariant failed.");

                var status = held > 0 ? "estimated" : "released";
                var usageRows = await db.UsageRecords
                    .Where(record => record.Id == id && record.BillingStatus == "settling")
                    .ExecuteUpdateAsync(update => update
                        .SetProperty(record => record.CostPoints, held)
                        .SetProperty(record => record.BillingStatus, status)
                        .SetProperty(record => record.RecordedAt, DateTime.UtcNow), ct);
                if (usageRows != 1)
                    throw new BillingInvariantException(
                        $"Reservation {usage.RequestId} lost its conservative settlement claim.");

                await tx.CommitAsync(ct);
                settled++;
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                await tx.RollbackAsync(CancellationToken.None);
                throw;
            }
            catch (Exception ex)
            {
                await tx.RollbackAsync(CancellationToken.None);
                failures ??= [];
                failures.Add(ex);
            }
        }

        if (failures is { Count: > 0 })
            throw new AggregateException(
                $"Failed to settle {failures.Count} stale streaming reservation(s).",
                failures);
        return settled;
    }

    public async Task<bool> ReleaseAsync(Guid userId, string requestId, CancellationToken ct)
    {
        await using var tx = await db.Database.BeginTransactionAsync(ct);
        try
        {
            var claimed = await db.UsageRecords
                .Where(r => r.UserId == userId
                            && r.RequestId == requestId
                            && r.BillingStatus == "pending")
                .ExecuteUpdateAsync(s => s.SetProperty(r => r.BillingStatus, "releasing"), ct);
            if (claimed != 1)
            {
                var exists = await db.UsageRecords.AsNoTracking()
                    .AnyAsync(r => r.UserId == userId && r.RequestId == requestId, ct);
                await tx.RollbackAsync(ct);
                if (!exists)
                    throw new BillingInvariantException($"Reservation {requestId} does not exist.");
                return false;
            }

            var pending = await db.UsageRecords.AsNoTracking()
                .SingleAsync(r => r.UserId == userId
                                  && r.RequestId == requestId
                                  && r.BillingStatus == "releasing", ct);
            await ReleaseClaimedAsync(pending, ct);
            await tx.CommitAsync(ct);
            return true;
        }
        catch
        {
            await tx.RollbackAsync(CancellationToken.None);
            throw;
        }
    }

    public async Task<int> ReleaseStaleAsync(DateTime cutoff, int batchSize, CancellationToken ct)
    {
        var ids = await db.UsageRecords.AsNoTracking()
            .Where(r => r.BillingStatus == "pending" && r.RecordedAt < cutoff)
            .OrderBy(r => r.RecordedAt)
            .Select(r => r.Id)
            .Take(Math.Clamp(batchSize, 1, 500))
            .ToListAsync(ct);

        var released = 0;
        List<Exception>? failures = null;
        foreach (var id in ids)
        {
            await using var tx = await db.Database.BeginTransactionAsync(ct);
            try
            {
                var claimed = await db.UsageRecords
                    .Where(r => r.Id == id
                                && r.BillingStatus == "pending"
                                && r.RecordedAt < cutoff)
                    .ExecuteUpdateAsync(s => s.SetProperty(r => r.BillingStatus, "releasing"), ct);
                if (claimed == 1)
                {
                    var pending = await db.UsageRecords.AsNoTracking()
                        .SingleAsync(r => r.Id == id && r.BillingStatus == "releasing", ct);
                    await ReleaseClaimedAsync(pending, ct);
                    await tx.CommitAsync(ct);
                    released++;
                }
                else
                {
                    await tx.RollbackAsync(ct);
                }
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                await tx.RollbackAsync(CancellationToken.None);
                throw;
            }
            catch (Exception ex)
            {
                await tx.RollbackAsync(CancellationToken.None);
                failures ??= [];
                failures.Add(ex);
                // One damaged account must not prevent unrelated stale holds
                // later in the batch from being released.
            }
        }
        if (failures is { Count: > 0 })
            throw new AggregateException(
                $"Failed to reconcile {failures.Count} stale billing reservation(s).",
                failures);
        return released;
    }

    private async Task ReleaseClaimedAsync(UsageRecord pending, CancellationToken ct)
    {
        var held = pending.ReservedPoints ?? 0;
        var userRows = await db.Users
            .Where(u => u.Id == pending.UserId && u.ReservedPoints >= held)
            .ExecuteUpdateAsync(s => s.SetProperty(
                u => u.ReservedPoints,
                u => u.ReservedPoints - held), ct);
        if (userRows != 1)
            throw new BillingInvariantException(
                $"Cannot release reservation {pending.RequestId}: user ledger invariant failed.");

        var usageRows = await db.UsageRecords
            .Where(r => r.Id == pending.Id && r.BillingStatus == "releasing")
            .ExecuteUpdateAsync(s => s
                .SetProperty(r => r.CostPoints, 0L)
                .SetProperty(r => r.BillingStatus, "released")
                .SetProperty(r => r.RecordedAt, DateTime.UtcNow), ct);
        if (usageRows != 1)
            throw new BillingInvariantException(
                $"Reservation {pending.RequestId} lost its release claim.");
    }

    private static ReservationResult ExistingReservation(UsageRecord usage)
    {
        var state = usage.BillingStatus is "pending" or "streaming" or "bill_pending" or "settling" or "releasing"
            ? ReservationState.AlreadyPending
            : ReservationState.AlreadyCompleted;
        return new ReservationResult(
            state, usage.Id, usage.ReservedPoints ?? 0, usage.BillingStatus);
    }

    private static SettlementResult ExistingSettlement(UsageRecord usage)
    {
        if (usage.BillingStatus is "pending" or "streaming" or "bill_pending" or "settling" or "releasing")
            throw new BillingInvariantException(
                $"Reservation {usage.RequestId} is not in a terminal state.");
        var held = usage.ReservedPoints ?? 0;
        var charged = usage.BillingStatus == "debt"
            ? Math.Min(usage.CostPoints, held)
            : usage.CostPoints;
        return new SettlementResult(
            usage.CostPoints,
            charged,
            Math.Max(0, usage.CostPoints - charged),
            usage.BillingStatus,
            false);
    }
}

public sealed class BillingInvariantException(string message) : Exception(message);
