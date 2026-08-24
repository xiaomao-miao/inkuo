using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Core.Billing;

/// <summary>
/// Durable fixed-price lifecycle for cloud web searches. Uses the same user
/// reserved-points invariant as chat billing, but keeps search audit rows in
/// their dedicated table.
/// </summary>
public sealed class WebSearchLedger(AppDbContext db)
{
    public enum ReservationState
    {
        Reserved,
        Duplicate,
        Rejected,
    }

    public sealed record ReservationResult(
        ReservationState State,
        string? BillingStatus,
        string? RejectionReason = null)
    {
        public bool CanForward => State == ReservationState.Reserved;
    }

    public async Task<ReservationResult> TryReserveAsync(
        Guid userId,
        string providerId,
        string query,
        long costPoints,
        string requestId,
        CancellationToken ct)
    {
        if (costPoints <= 0) throw new ArgumentOutOfRangeException(nameof(costPoints));
        if (string.IsNullOrWhiteSpace(requestId))
            throw new ArgumentException("A request id is required.", nameof(requestId));

        var existing = await db.WebSearchUsageRecords.AsNoTracking()
            .SingleOrDefaultAsync(
                record => record.UserId == userId && record.RequestId == requestId,
                ct);
        if (existing is not null)
            return new ReservationResult(ReservationState.Duplicate, existing.BillingStatus);

        await using var tx = await db.Database.BeginTransactionAsync(ct);
        try
        {
            var userRows = await db.Users
                .Where(user => user.Id == userId
                               && !user.IsSuspended
                               && user.BalancePoints >= 0
                               && user.ReservedPoints >= 0
                               && user.ReservedPoints <= user.BalancePoints
                               && user.BalancePoints - user.ReservedPoints >= costPoints)
                .ExecuteUpdateAsync(update => update.SetProperty(
                    user => user.ReservedPoints,
                    user => user.ReservedPoints + costPoints), ct);
            if (userRows != 1)
            {
                await tx.RollbackAsync(ct);
                var account = await db.Users.AsNoTracking()
                    .Where(user => user.Id == userId)
                    .Select(user => new
                    {
                        user.IsSuspended,
                        user.AdminSuspended,
                        user.DebtPoints,
                    })
                    .SingleOrDefaultAsync(ct);
                var reason = account is null
                    ? "account_unavailable"
                    : account.AdminSuspended
                        ? "admin_suspended"
                        : account.IsSuspended || account.DebtPoints > 0
                            ? "billing_suspended"
                            : "insufficient_points";
                return new ReservationResult(ReservationState.Rejected, null, reason);
            }

            db.WebSearchUsageRecords.Add(new WebSearchUsageRecord
            {
                UserId = userId,
                ProviderId = providerId.Trim().ToLowerInvariant(),
                Query = query.Length <= 512 ? query : query[..512],
                CostPoints = costPoints,
                ReservedPoints = costPoints,
                RequestId = requestId,
                BillingStatus = "pending",
                RecordedAt = DateTime.UtcNow,
            });
            await db.SaveChangesAsync(ct);
            await tx.CommitAsync(ct);
            return new ReservationResult(ReservationState.Reserved, "pending");
        }
        catch (DbUpdateException)
        {
            await tx.RollbackAsync(CancellationToken.None);
            db.ChangeTracker.Clear();
            var winner = await db.WebSearchUsageRecords.AsNoTracking()
                .SingleOrDefaultAsync(
                    record => record.UserId == userId && record.RequestId == requestId,
                    ct);
            if (winner is not null)
                return new ReservationResult(ReservationState.Duplicate, winner.BillingStatus);
            throw;
        }
        catch
        {
            await tx.RollbackAsync(CancellationToken.None);
            throw;
        }
    }

    public async Task<bool> MarkStartedAsync(
        Guid userId,
        string requestId,
        CancellationToken ct)
    {
        var rows = await db.WebSearchUsageRecords
            .Where(record => record.UserId == userId
                             && record.RequestId == requestId
                             && record.BillingStatus == "pending")
            .ExecuteUpdateAsync(update => update
                .SetProperty(record => record.BillingStatus, "started")
                .SetProperty(record => record.RecordedAt, DateTime.UtcNow), ct);
        return rows == 1;
    }

    public async Task<bool> SettleAsync(
        Guid userId,
        string requestId,
        CancellationToken ct)
    {
        await using var tx = await db.Database.BeginTransactionAsync(ct);
        try
        {
            var claimed = await db.WebSearchUsageRecords
                .Where(record => record.UserId == userId
                                 && record.RequestId == requestId
                                 && record.BillingStatus == "started")
                .ExecuteUpdateAsync(update => update.SetProperty(
                    record => record.BillingStatus,
                    "settling"), ct);
            if (claimed != 1)
            {
                var status = await db.WebSearchUsageRecords.AsNoTracking()
                    .Where(record => record.UserId == userId && record.RequestId == requestId)
                    .Select(record => record.BillingStatus)
                    .SingleOrDefaultAsync(ct);
                await tx.RollbackAsync(ct);
                if (status == "settled") return false;
                throw new BillingInvariantException(
                    $"Web search reservation {requestId} cannot be settled from status {status ?? "missing"}.");
            }

            var usage = await db.WebSearchUsageRecords.AsNoTracking()
                .SingleAsync(record => record.UserId == userId
                                       && record.RequestId == requestId
                                       && record.BillingStatus == "settling", ct);
            var held = usage.ReservedPoints ?? usage.CostPoints;
            var userRows = await db.Users
                .Where(user => user.Id == userId
                               && user.ReservedPoints >= held
                               && user.BalancePoints >= held)
                .ExecuteUpdateAsync(update => update
                    .SetProperty(user => user.BalancePoints, user => user.BalancePoints - held)
                    .SetProperty(user => user.ReservedPoints, user => user.ReservedPoints - held), ct);
            if (userRows != 1)
                throw new BillingInvariantException(
                    $"Cannot settle web search {requestId}: user ledger invariant failed.");

            var usageRows = await db.WebSearchUsageRecords
                .Where(record => record.Id == usage.Id && record.BillingStatus == "settling")
                .ExecuteUpdateAsync(update => update
                    .SetProperty(record => record.CostPoints, held)
                    .SetProperty(record => record.BillingStatus, "settled")
                    .SetProperty(record => record.RecordedAt, DateTime.UtcNow), ct);
            if (usageRows != 1)
                throw new BillingInvariantException(
                    $"Web search reservation {requestId} lost its settlement claim.");

            await tx.CommitAsync(ct);
            return true;
        }
        catch
        {
            await tx.RollbackAsync(CancellationToken.None);
            throw;
        }
    }

    public async Task<bool> ReleaseAsync(
        Guid userId,
        string requestId,
        CancellationToken ct)
    {
        await using var tx = await db.Database.BeginTransactionAsync(ct);
        try
        {
            var claimed = await db.WebSearchUsageRecords
                .Where(record => record.UserId == userId
                                 && record.RequestId == requestId
                                 && (record.BillingStatus == "pending"
                                     || record.BillingStatus == "started"))
                .ExecuteUpdateAsync(update => update.SetProperty(
                    record => record.BillingStatus,
                    "releasing"), ct);
            if (claimed != 1)
            {
                var exists = await db.WebSearchUsageRecords.AsNoTracking()
                    .AnyAsync(
                        record => record.UserId == userId && record.RequestId == requestId,
                        ct);
                await tx.RollbackAsync(ct);
                if (!exists)
                    throw new BillingInvariantException(
                        $"Web search reservation {requestId} does not exist.");
                return false;
            }

            var usage = await db.WebSearchUsageRecords.AsNoTracking()
                .SingleAsync(record => record.UserId == userId
                                       && record.RequestId == requestId
                                       && record.BillingStatus == "releasing", ct);
            await ReleaseClaimedAsync(usage, ct);
            await tx.CommitAsync(ct);
            return true;
        }
        catch
        {
            await tx.RollbackAsync(CancellationToken.None);
            throw;
        }
    }

    public async Task<int> SettleStaleStartedAsync(
        DateTime cutoff,
        int batchSize,
        CancellationToken ct)
    {
        var rows = await db.WebSearchUsageRecords.AsNoTracking()
            .Where(record => record.BillingStatus == "started" && record.RecordedAt < cutoff)
            .OrderBy(record => record.RecordedAt)
            .Select(record => new { record.UserId, record.RequestId })
            .Take(Math.Clamp(batchSize, 1, 500))
            .ToListAsync(ct);
        var settled = 0;
        foreach (var row in rows)
        {
            if (row.RequestId is not null
                && await SettleAsync(row.UserId, row.RequestId, ct))
                settled++;
        }
        return settled;
    }

    public async Task<int> ReleaseStalePendingAsync(
        DateTime cutoff,
        int batchSize,
        CancellationToken ct)
    {
        var ids = await db.WebSearchUsageRecords.AsNoTracking()
            .Where(record => record.BillingStatus == "pending" && record.RecordedAt < cutoff)
            .OrderBy(record => record.RecordedAt)
            .Select(record => record.Id)
            .Take(Math.Clamp(batchSize, 1, 500))
            .ToListAsync(ct);
        var released = 0;
        foreach (var id in ids)
        {
            await using var tx = await db.Database.BeginTransactionAsync(ct);
            try
            {
                var claimed = await db.WebSearchUsageRecords
                    .Where(record => record.Id == id
                                     && record.BillingStatus == "pending"
                                     && record.RecordedAt < cutoff)
                    .ExecuteUpdateAsync(update => update.SetProperty(
                        record => record.BillingStatus,
                        "releasing"), ct);
                if (claimed != 1)
                {
                    await tx.RollbackAsync(ct);
                    continue;
                }

                var usage = await db.WebSearchUsageRecords.AsNoTracking()
                    .SingleAsync(record => record.Id == id && record.BillingStatus == "releasing", ct);
                await ReleaseClaimedAsync(usage, ct);
                await tx.CommitAsync(ct);
                released++;
            }
            catch
            {
                await tx.RollbackAsync(CancellationToken.None);
                throw;
            }
        }
        return released;
    }

    private async Task ReleaseClaimedAsync(WebSearchUsageRecord usage, CancellationToken ct)
    {
        var held = usage.ReservedPoints ?? 0;
        var userRows = await db.Users
            .Where(user => user.Id == usage.UserId && user.ReservedPoints >= held)
            .ExecuteUpdateAsync(update => update.SetProperty(
                user => user.ReservedPoints,
                user => user.ReservedPoints - held), ct);
        if (userRows != 1)
            throw new BillingInvariantException(
                $"Cannot release web search {usage.RequestId}: user ledger invariant failed.");

        var usageRows = await db.WebSearchUsageRecords
            .Where(record => record.Id == usage.Id && record.BillingStatus == "releasing")
            .ExecuteUpdateAsync(update => update
                .SetProperty(record => record.CostPoints, 0L)
                .SetProperty(record => record.BillingStatus, "released")
                .SetProperty(record => record.RecordedAt, DateTime.UtcNow), ct);
        if (usageRows != 1)
            throw new BillingInvariantException(
                $"Web search reservation {usage.RequestId} lost its release claim.");
    }
}
