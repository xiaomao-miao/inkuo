using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminUsersEndpoints
{
    public record UserListItem(
        Guid Id, string Email, decimal BalanceCents, DateTime CreatedAt,
        string InviteCodeUsed, string? PlanName, DateTime? SubExpiresAt,
        long TotalTokens, decimal TotalCostCents, int SubscriptionCount, bool Disabled);

    public record UserDetail(
        Guid Id, string Email, DateTime CreatedAt, string InviteCodeUsed, decimal BalanceCents);

    public record AdjustBalanceRequest(decimal DeltaCents, string Reason);

    public record DisableRequest(bool Disabled);

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
                ("balance", "asc") => query.OrderBy(u => u.BalanceCents),
                ("balance", "desc") => query.OrderByDescending(u => u.BalanceCents),
                ("createdat", "asc") => query.OrderBy(u => u.CreatedAt),
                _ => query.OrderByDescending(u => u.CreatedAt),
            };

            var total = await query.CountAsync();

            var rows = await query
                .Skip((page - 1) * pageSize)
                .Take(pageSize)
                .Select(u => new
                {
                    u.Id, u.Email, u.BalanceCents, u.CreatedAt, u.InviteCodeUsed,
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
                .Where(u => ids.Contains(u.UserId))
                .GroupBy(u => u.UserId)
                .Select(g => new
                {
                    UserId = g.Key,
                    Tokens = g.Sum(u => (long)u.PromptTokens + u.CompletionTokens),
                    Cost = g.Sum(u => u.CostCents),
                })
                .ToDictionaryAsync(g => g.UserId, g => new { g.Tokens, g.Cost });

            var items = rows.Select(r => new UserListItem(
                r.Id, r.Email, r.BalanceCents, r.CreatedAt, r.InviteCodeUsed ?? "",
                subs.TryGetValue(r.Id, out var s) ? s.PlanName : null,
                subs.TryGetValue(r.Id, out var s2) ? s2.ExpiresAt : null,
                usage.TryGetValue(r.Id, out var u) ? u.Tokens : 0L,
                usage.TryGetValue(r.Id, out var u2) ? u2.Cost : 0m,
                subs.ContainsKey(r.Id) ? 1 : 0,
                false)).ToList();

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
                .Where(u => u.UserId == id)
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
                user = new UserDetail(user.Id, user.Email, user.CreatedAt, user.InviteCodeUsed ?? "", user.BalanceCents),
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
                    costCents = usage.Sum(u => u.CostCents),
                    recordCount = await db.UsageRecords.CountAsync(r => r.UserId == id),
                },
                recentUsage = usage.Select(u => new
                {
                    u.Id,
                    ModelName = u.ModelConfig.DisplayName,
                    u.PromptTokens,
                    u.CompletionTokens,
                    u.CostCents,
                    u.RecordedAt,
                }),
                refreshTokens,
            });
        });

        // Adjust balance manually (gift / refund)
        group.MapPost("/{id:guid}/adjust-balance", async (Guid id, AdjustBalanceRequest req, AppDbContext db) =>
        {
            if (string.IsNullOrWhiteSpace(req.Reason))
                return Results.BadRequest(new { error = "Reason is required for audit trail" });

            var user = await db.Users.FindAsync(id);
            if (user == null) return Results.NotFound();

            user.BalanceCents += req.DeltaCents;
            await db.SaveChangesAsync();

            return Results.Ok(new
            {
                newBalanceCents = user.BalanceCents,
                deltaCents = req.DeltaCents,
                reason = req.Reason,
            });
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
            var tokens = await db.RefreshTokens.Where(rt => rt.UserId == id).ToListAsync();

            db.Subscriptions.RemoveRange(subs);
            db.UsageRecords.RemoveRange(usage);
            db.RefreshTokens.RemoveRange(tokens);
            db.Users.Remove(user);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "User deleted", id });
        });
    }
}