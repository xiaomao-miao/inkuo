using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Billing;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminUsersEndpoints
{
    public record UserListItem(
        Guid Id, string Email, long BalancePoints, long ReservedPoints, long DebtPoints, bool IsSuspended,
        DateTime CreatedAt, string InviteCodeUsed, string? PlanName, DateTime? SubExpiresAt,
        long TotalTokens, long TotalCostPoints, int SubscriptionCount);

    public record UserDetail(
        Guid Id, string Email, DateTime CreatedAt, string InviteCodeUsed,
        long BalancePoints, long ReservedPoints, long DebtPoints, bool IsSuspended);

    public record AdjustBalanceRequest(long DeltaPoints, string Reason);

    public record SuspendRequest(bool Suspended);

    public static void MapAdminUsersEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/users").WithTags("users").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db, int page = 1, int pageSize = 20,
            string? search = null, string? sortBy = "createdAt", string? sortDir = "desc") =>
        {
            page = Math.Max(1, page);
            pageSize = Math.Clamp(pageSize, 1, 100);

            var query = db.Users.AsQueryable();
            if (!string.IsNullOrWhiteSpace(search))
                query = query.Where(u => u.Email.Contains(search));

            query = (sortBy?.ToLowerInvariant(), sortDir?.ToLowerInvariant()) switch
            {
                ("email", "asc") => query.OrderBy(u => u.Email),
                ("email", "desc") => query.OrderByDescending(u => u.Email),
                ("balance", "asc") => query.OrderBy(u => u.BalancePoints),
                ("balance", "desc") => query.OrderByDescending(u => u.BalancePoints),
                ("createdat", "asc") => query.OrderBy(u => u.CreatedAt),
                _ => query.OrderByDescending(u => u.CreatedAt),
            };

            var total = await query.CountAsync();

            var rows = await query
                .Skip((page - 1) * pageSize)
                .Take(pageSize)
                .Select(u => new
                {
                    u.Id, u.Email, u.BalancePoints, u.ReservedPoints, u.DebtPoints, u.IsSuspended,
                    u.CreatedAt, u.InviteCodeUsed,
                })
                .ToListAsync();

            var ids = rows.Select(r => r.Id).ToList();
            var subs = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => ids.Contains(s.UserId) && s.Status == "active")
                .GroupBy(s => s.UserId)
                .Select(g => g.OrderByDescending(s => s.ExpiresAt).First())
                .ToDictionaryAsync(
                    s => s.UserId,
                    s => new { PlanName = s.Plan.Name, ExpiresAt = (DateTime?)s.ExpiresAt });

            var usage = await db.UsageRecords
                .Where(u => ids.Contains(u.UserId)
                            && (u.BillingStatus == "settled"
                                || u.BillingStatus == "released"
                                || u.BillingStatus == "debt"
                                || u.BillingStatus == "estimated"))
                .GroupBy(u => u.UserId)
                .Select(g => new
                {
                    UserId = g.Key,
                    Tokens = g.Sum(u => (long)u.PromptTokens + u.CompletionTokens),
                    Cost = g.Sum(u => u.CostPoints),
                })
                .ToDictionaryAsync(g => g.UserId, g => new { g.Tokens, g.Cost });

            var items = rows.Select(r => new UserListItem(
                r.Id, r.Email, r.BalancePoints, r.ReservedPoints, r.DebtPoints, r.IsSuspended,
                r.CreatedAt, r.InviteCodeUsed ?? "",
                subs.TryGetValue(r.Id, out var s) ? s.PlanName : null,
                subs.TryGetValue(r.Id, out var s2) ? s2.ExpiresAt : null,
                usage.TryGetValue(r.Id, out var u) ? u.Tokens : 0L,
                usage.TryGetValue(r.Id, out var u2) ? u2.Cost : 0L,
                subs.ContainsKey(r.Id) ? 1 : 0)).ToList();

            return Results.Ok(new { total, page, pageSize, items });
        });

        group.MapGet("/{id:guid}", async (Guid id, AppDbContext db) =>
        {
            var user = await db.Users.FindAsync(id);
            if (user == null) return Results.NotFound();

            var subs = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => s.UserId == id)
                .OrderByDescending(s => s.StartedAt)
                .ToListAsync();

            var usage = await db.UsageRecords
                .Include(u => u.ModelConfig)
                .Where(u => u.UserId == id
                            && (u.BillingStatus == "settled"
                                || u.BillingStatus == "released"
                                || u.BillingStatus == "debt"
                                || u.BillingStatus == "estimated"))
                .OrderByDescending(u => u.RecordedAt)
                .Take(100)
                .ToListAsync();

            var refreshTokens = await db.RefreshTokens
                .Where(rt => rt.UserId == id)
                .OrderByDescending(rt => rt.ExpiresAt)
                .Take(20)
                .Select(rt => new { rt.Jti, rt.ExpiresAt, rt.Revoked })
                .ToListAsync();

            return Results.Ok(new
            {
                user = new UserDetail(
                    user.Id, user.Email, user.CreatedAt, user.InviteCodeUsed ?? "",
                    user.BalancePoints, user.ReservedPoints, user.DebtPoints, user.IsSuspended),
                subscriptions = subs.Select(s => new
                {
                    s.Id,
                    PlanName = s.Plan.Name,
                    s.StartedAt,
                    s.ExpiresAt,
                    s.Status,
                }),
                totalUsage = new
                {
                    tokens = usage.Sum(u => u.PromptTokens + u.CompletionTokens),
                    costPoints = usage.Sum(u => u.CostPoints),
                    recordCount = await db.UsageRecords.CountAsync(r => r.UserId == id),
                },
                recentUsage = usage.Select(u => new
                {
                    u.Id,
                    ModelName = u.ModelConfig.DisplayName,
                    u.PromptTokens,
                    u.CompletionTokens,
                    u.CostPoints,
                    u.BillingStatus,
                    u.RecordedAt,
                }),
                refreshTokens,
            });
        });

        // Adjust balance manually (gift / refund). Uses a conditional UPDATE so
        // the audit row's read never sees a stale snapshot in a concurrent
        // settlement.
        group.MapPost("/{id:guid}/adjust-balance", async (
            Guid id,
            AdjustBalanceRequest req,
            AppDbContext db,
            HttpContext context,
            ILoggerFactory loggerFactory) =>
        {
            var reason = (req.Reason ?? string.Empty).Trim();
            if (reason.Length < 3 || reason.Length > BillingLimits.MaxAdjustmentReasonLength)
                return Results.BadRequest(new
                {
                    error = $"Reason must be between 3 and {BillingLimits.MaxAdjustmentReasonLength} characters",
                });
            if (req.DeltaPoints == 0
                || req.DeltaPoints < -BillingLimits.MaxSingleCreditPoints
                || req.DeltaPoints > BillingLimits.MaxSingleCreditPoints)
                return Results.BadRequest(new
                {
                    error = $"DeltaPoints must be non-zero and between {-BillingLimits.MaxSingleCreditPoints} and {BillingLimits.MaxSingleCreditPoints}",
                });

            var user = await db.Users.FindAsync(id);
            if (user == null) return Results.NotFound();

            var positiveCredit = req.DeltaPoints > 0
                ? await AccountCredit.ApplyAsync(db, id, req.DeltaPoints, CancellationToken.None)
                : null;
            var debitPoints = req.DeltaPoints < 0 ? -req.DeltaPoints : 0;
            var updated = req.DeltaPoints > 0
                ? positiveCredit is null ? 0 : 1
                : await db.Users
                    .Where(u => u.Id == id
                                && u.BalancePoints >= 0
                                && u.BalancePoints <= BillingLimits.MaxAccountBalancePoints
                                && u.ReservedPoints >= 0
                                && u.BalancePoints >= debitPoints
                                && u.ReservedPoints <= u.BalancePoints - debitPoints)
                    .ExecuteUpdateAsync(s => s.SetProperty(
                        u => u.BalancePoints,
                        u => u.BalancePoints - debitPoints));
            if (updated != 1)
                return Results.BadRequest(new
                {
                    error = $"Resulting balance must stay between reserved points and the {BillingLimits.MaxAccountBalancePoints}-point account limit",
                });

            var newLedger = positiveCredit is not null
                ? new
                {
                    positiveCredit.BalancePoints,
                    positiveCredit.DebtPoints,
                    positiveCredit.IsSuspended,
                }
                : await db.Users
                    .Where(u => u.Id == id)
                    .Select(u => new { u.BalancePoints, u.DebtPoints, u.IsSuspended })
                    .SingleAsync();

            loggerFactory.CreateLogger("Inkuso.Cloud.Admin.Balance")
                .LogWarning(
                    "Admin {Admin} adjusted user {UserId} by {DeltaPoints} points: {Reason}",
                    context.User.Identity?.Name ?? "unknown",
                    id,
                    req.DeltaPoints,
                    reason);

            return Results.Ok(new
            {
                newLedger.BalancePoints,
                newLedger.DebtPoints,
                newLedger.IsSuspended,
                deltaPoints = req.DeltaPoints,
                reason,
            });
        });

        // Toggle suspension caused by an unpaid billing event. Unsetting
        // requires an explicit confirmation so an admin can't accidentally
        // release a debt recorder.
        group.MapPost("/{id:guid}/suspend", async (Guid id, SuspendRequest req, AppDbContext db) =>
        {
            var user = await db.Users.FindAsync(id);
            if (user == null) return Results.NotFound();
            if (!req.Suspended && user.DebtPoints > 0)
                return Results.BadRequest(new
                {
                    error = "Outstanding debt must be paid or explicitly adjusted before unsuspending this account",
                    debtPoints = user.DebtPoints,
                });
            user.AdminSuspended = req.Suspended;
            user.IsSuspended = req.Suspended || user.DebtPoints > 0;
            await db.SaveChangesAsync();
            return Results.Ok(new { id = user.Id, isSuspended = user.IsSuspended });
        });

        // Revoke all refresh tokens for the user (force logout)
        group.MapPost("/{id:guid}/revoke-sessions", async (Guid id, AppDbContext db) =>
        {
            var tokens = await db.RefreshTokens
                .Where(rt => rt.UserId == id && !rt.Revoked)
                .ToListAsync();
            foreach (var t in tokens) t.Revoked = true;
            await db.SaveChangesAsync();
            return Results.Ok(new { revoked = tokens.Count });
        });

        // Delete user (admin escalation; removes everything)
        group.MapDelete("/{id:guid}", async (Guid id, AppDbContext db) =>
        {
            var user = await db.Users.FindAsync(id);
            if (user == null) return Results.NotFound();

            var subs = await db.Subscriptions.Where(s => s.UserId == id).ToListAsync();
            var usage = await db.UsageRecords.Where(u => u.UserId == id).ToListAsync();
            var webSearchUsage = await db.WebSearchUsageRecords
                .Where(u => u.UserId == id)
                .ToListAsync();
            var tokens = await db.RefreshTokens.Where(rt => rt.UserId == id).ToListAsync();

            db.Subscriptions.RemoveRange(subs);
            db.UsageRecords.RemoveRange(usage);
            db.WebSearchUsageRecords.RemoveRange(webSearchUsage);
            db.RefreshTokens.RemoveRange(tokens);
            db.Users.Remove(user);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "User deleted", id });
        });
    }
}
