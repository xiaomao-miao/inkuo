using System.Security.Claims;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Redeem
{
    public record RedeemRequest(string Code);
    public record RedeemResult(string Message, decimal NewBalanceCents, string? PlanName);

    public static void MapRedeemEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/redeem").WithTags("redeem").RequireAuthorization();

        group.MapPost("/", async (RedeemRequest req, HttpContext ctx, AppDbContext db) =>
        {
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);

            // Atomically reserve a use of the redemption code. The conditional
            // UPDATE guards against two concurrent callers both passing the
            // UsedCount < MaxUses check that the previous implementation did
            // in C# — the original bug let two users redeem a single-use code
            // if they raced within the same millisecond.
            var reservation = await db.RedemptionCodes
                .Where(r => r.Code == req.Code && r.Enabled
                    && (r.ExpiresAt == null || r.ExpiresAt > DateTime.UtcNow)
                    && r.UsedCount < r.MaxUses)
                .ExecuteUpdateAsync(s => s.SetProperty(r => r.UsedCount, r => r.UsedCount + 1));

            if (reservation == 0)
                return Results.BadRequest(new { error = "Invalid or exhausted redemption code" });

            var code = await db.RedemptionCodes
                .Include(r => r.Plan)
                .FirstOrDefaultAsync(r => r.Code == req.Code);

            // We just bumped UsedCount, so even if the reservation above
            // succeeded the row might still be missing if a parallel admin
            // hard-deleted it. Treat that as "no longer valid".
            if (code is null)
                return Results.BadRequest(new { error = "Invalid or exhausted redemption code" });

            string? newPlan = null;

            if (code.PlanId != null && code.Plan != null)
            {
                var existingSub = await db.Subscriptions
                    .Where(s => s.UserId == userId && s.Status == "active" && s.ExpiresAt > DateTime.UtcNow)
                    .OrderByDescending(s => s.ExpiresAt)
                    .FirstOrDefaultAsync();

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
            }

            var user = await db.Users.FindAsync(userId);
            if (user != null && code.CreditCents > 0)
            {
                user.BalanceCents += code.CreditCents;
            }

            await db.SaveChangesAsync();

            return Results.Ok(new RedeemResult(
                code.PlanId != null ? "Subscription activated" : "Credit added",
                user?.BalanceCents ?? 0,
                newPlan));
        });
    }
}
