using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Core.Billing;

public static class AccountCredit
{
    public sealed record Result(
        long BalancePoints,
        long DebtPoints,
        bool IsSuspended);

    /// <summary>
    /// Applies positive credit atomically: outstanding usage is paid first,
    /// then only the remainder becomes spendable balance. A debt suspension is
    /// cleared when fully paid, while an unrelated manual suspension remains.
    /// </summary>
    public static async Task<Result?> ApplyAsync(
        AppDbContext db,
        Guid userId,
        long creditPoints,
        CancellationToken ct)
    {
        if (creditPoints <= 0 || creditPoints > BillingLimits.MaxSingleCreditPoints)
            throw new ArgumentOutOfRangeException(
                nameof(creditPoints),
                $"Credit must be between 1 and {BillingLimits.MaxSingleCreditPoints} points.");

        var updated = await db.Users
            .Where(user => user.Id == userId
                           && user.BalancePoints >= 0
                           && user.BalancePoints <= BillingLimits.MaxAccountBalancePoints
                           && user.DebtPoints >= 0
                           && (user.DebtPoints >= creditPoints
                               || user.BalancePoints <= BillingLimits.MaxAccountBalancePoints
                                  - (creditPoints - user.DebtPoints)))
            .ExecuteUpdateAsync(update => update
                .SetProperty(
                    user => user.BalancePoints,
                    user => user.BalancePoints
                            + (creditPoints > user.DebtPoints
                                ? creditPoints - user.DebtPoints
                                : 0))
                .SetProperty(
                    user => user.DebtPoints,
                    user => user.DebtPoints > creditPoints
                        ? user.DebtPoints - creditPoints
                        : 0)
                .SetProperty(
                    user => user.IsSuspended,
                    user => user.AdminSuspended
                            || user.DebtPoints > creditPoints), ct);
        if (updated != 1) return null;

        return await db.Users.AsNoTracking()
            .Where(user => user.Id == userId)
            .Select(user => new Result(
                user.BalancePoints,
                user.DebtPoints,
                user.IsSuspended))
            .SingleAsync(ct);
    }
}
