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

    public async Task<ReservationResult> TryReserveAsync(
        Guid userId,
        Guid modelConfigId,
        long pointsToReserve,
        string requestId,
        CancellationToken ct)
    {
        if (string.IsNullOrWhiteSpace(requestId))
            throw new ArgumentException("A request id is required for billing idempotency.", nameof(requestId));
        if (pointsToReserve < 0)
            throw new ArgumentOutOfRangeException(nameof(pointsToReserve));

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
        catch
        {
            await tx.RollbackAsync(ct);
            throw;
        }
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
                            && r.BillingStatus == "pending")
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

            var held = pending.ReservedPoints ?? 0;
            var actual = LlmForwarder.CalculateCostPoints(
                config, promptTokens, completionTokens, cachedPromptTokens);
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
            await tx.RollbackAsync(ct);
            throw;
        }
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
            await tx.RollbackAsync(ct);
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
            catch
            {
                await tx.RollbackAsync(ct);
                throw;
            }
        }
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
        var state = usage.BillingStatus == "pending"
            ? ReservationState.AlreadyPending
            : ReservationState.AlreadyCompleted;
        return new ReservationResult(
            state, usage.Id, usage.ReservedPoints ?? 0, usage.BillingStatus);
    }

    private static SettlementResult ExistingSettlement(UsageRecord usage)
    {
        if (usage.BillingStatus is "pending" or "settling" or "releasing")
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
