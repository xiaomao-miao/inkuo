using System.Security.Claims;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Redeem
{
    public record RedeemRequest(string Code);
    public record RedeemResult(
        string Message,
        long NewBalancePoints,
        long GrantedPoints,
        long RemainingDebtPoints,
        bool IsSuspended,
        string? PlanName,
        int? SubscriptionDaysAdded);

    public static void MapRedeemEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/redeem").WithTags("redeem").RequireAuthorization();

        group.MapPost("/", async (
            RedeemRequest req,
            HttpContext ctx,
            AppDbContext db,
            CancellationToken ct) =>
        {
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);
            var normalizedCode = (req.Code ?? string.Empty).Trim();
            if (normalizedCode.Length == 0)
                return Results.BadRequest(new { error = "Code is required" });

            var user = await db.Users.FindAsync(new object[] { userId }, ct);
            if (user is null) return Results.Unauthorized();

            await using var tx = await db.Database.BeginTransactionAsync(ct);
            try
            {

                // The use counter, subscription, and credit grant commit as one
                // unit. A later write failure must not consume a limited code.
                var reservation = await db.RedemptionCodes
                    .Where(r => r.Code == normalizedCode && r.Enabled
                        && (r.ExpiresAt == null || r.ExpiresAt > DateTime.UtcNow)
                        && r.UsedCount < r.MaxUses)
                    .ExecuteUpdateAsync(s => s.SetProperty(
                        r => r.UsedCount,
                        r => r.UsedCount + 1), ct);

                if (reservation == 0)
                {
                    await tx.RollbackAsync(ct);
                    return Results.BadRequest(new { error = "Invalid or exhausted redemption code" });
                }

                var code = await db.RedemptionCodes.AsNoTracking()
                    .Include(r => r.Plan)
                    .FirstOrDefaultAsync(r => r.Code == normalizedCode, ct);

                // We just bumped UsedCount, so this should be impossible unless
                // the row was removed by an administrative race.
                if (code is null)
                    throw new InvalidOperationException("Reserved redemption code disappeared.");
                if (BillingLimits.ValidatePointGrant(code.CreditPoints, allowZero: true) is not null)
                {
                    await tx.RollbackAsync(ct);
                    return Results.Json(
                        new { error = "Redemption code is temporarily unavailable; contact support." },
                        statusCode: 503);
                }
                if (code.CreditPoints == 0 && (code.PlanId is null || code.Plan is null))
                {
                    await tx.RollbackAsync(ct);
                    return Results.Json(
                        new { error = "Redemption code is temporarily unavailable; contact support." },
                        statusCode: 503);
                }

                long grantedPoints = 0;
                int? subscriptionDaysAdded = null;

                // --- Plan grant ---
                string? newPlan = null;
                if (code.PlanId != null && code.Plan != null)
                {
                    var existingSub = await db.Subscriptions
                        .Where(s => s.UserId == userId
                                    && s.Status == "active"
                                    && s.ExpiresAt > DateTime.UtcNow)
                        .OrderByDescending(s => s.ExpiresAt)
                        .FirstOrDefaultAsync(ct);

                    var startAt = existingSub != null && existingSub.ExpiresAt > DateTime.UtcNow
                        ? existingSub.ExpiresAt
                        : DateTime.UtcNow;

                    db.Subscriptions.Add(new Core.Entities.Subscription
                    {
                        UserId = userId,
                        PlanId = code.Plan.Id,
                        StartedAt = startAt,
                        ExpiresAt = startAt.AddMonths(1),
                        Status = "active",
                    });

                    newPlan = code.Plan.Name;
                    subscriptionDaysAdded = 30;
                }

                // --- Credit grant (atomic add) ---
                if (code.CreditPoints > 0)
                {
                    grantedPoints = code.CreditPoints;
                    var credited = await AccountCredit.ApplyAsync(
                        db,
                        userId,
                        grantedPoints,
                        ct);
                    if (credited is null)
                    {
                        await tx.RollbackAsync(ct);
                        return Results.Conflict(new
                        {
                            error = "account_balance_limit",
                            message = "This credit would exceed the account balance limit. Contact support before retrying.",
                        });
                    }
                }

                await db.SaveChangesAsync(ct);
                await tx.CommitAsync(ct);

                // ExecuteUpdate bypasses the tracked entity; reload after commit
                // so the response reports the actual concurrent-safe balance.
                await db.Entry(user).ReloadAsync(ct);

                var message = code.PlanId != null
                    ? "Subscription activated"
                    : user.DebtPoints > 0
                        ? "Credit applied to outstanding usage; account remains suspended"
                        : "Credit added; account active";
                return Results.Ok(new RedeemResult(
                    message,
                    user.BalancePoints,
                    grantedPoints,
                    user.DebtPoints,
                    user.IsSuspended,
                    newPlan,
                    subscriptionDaysAdded));
            }
            catch
            {
                await tx.RollbackAsync(CancellationToken.None);
                throw;
            }
        });
    }
}
