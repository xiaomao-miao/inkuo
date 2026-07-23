using System.Security.Claims;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Account
{
    public record AccountInfo(Guid Id, string Email, decimal BalanceCents,
        string? PlanName, long MonthlyTokenLimit, DateTime? SubscriptionExpiresAt,
        long TokensUsedThisMonth, long MonthlyTokensRemaining);

    public static void MapAccountEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/account").WithTags("account").RequireAuthorization();

        group.MapGet("/me", async (HttpContext ctx, AppDbContext db) =>
        {
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);

            var user = await db.Users.FindAsync(userId);
            if (user == null) return Results.NotFound();

            var sub = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => s.UserId == userId && s.Status == "active" && s.ExpiresAt > DateTime.UtcNow)
                .OrderByDescending(s => s.ExpiresAt)
                .FirstOrDefaultAsync();

            // Token usage this calendar month. Use Sum twice (rather than a
            // single `prompt + completion` expression) because the Npgsql
            // translator occasionally promotes `long? + long` to text in
            // older provider versions, which produces a cast exception. Two
            // straight SUM aggregates are unambiguous and let Postgres use
            // the IX_UsageRecords_UserId_RecordedAt index.
            var startOfMonth = new DateTime(DateTime.UtcNow.Year, DateTime.UtcNow.Month, 1, 0, 0, 0, DateTimeKind.Utc);
            var promptTokens = await db.UsageRecords
                .Where(u => u.UserId == userId && u.RecordedAt >= startOfMonth)
                .SumAsync(u => (long?)u.PromptTokens) ?? 0L;
            var completionTokens = await db.UsageRecords
                .Where(u => u.UserId == userId && u.RecordedAt >= startOfMonth)
                .SumAsync(u => (long?)u.CompletionTokens) ?? 0L;
            var tokensUsed = promptTokens + completionTokens;

            var limit = sub?.Plan.MonthlyTokenLimit ?? 500_000;
            var remaining = Math.Max(0, limit - tokensUsed);

            return Results.Ok(new AccountInfo(
                user.Id, user.Email, user.BalanceCents,
                sub?.Plan.Name, limit,
                sub?.ExpiresAt,
                tokensUsed, remaining
            ));
        });

        group.MapGet("/usage", async (HttpContext ctx, AppDbContext db) =>
        {
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);

            var records = await db.UsageRecords
                .Include(u => u.ModelConfig)
                .Where(u => u.UserId == userId)
                .OrderByDescending(u => u.RecordedAt)
                .Take(50)
                .Select(u => new
                {
                    u.Id,
                    Model = u.ModelConfig.DisplayName,
                    u.PromptTokens,
                    u.CompletionTokens,
                    u.CostCents,
                    u.RecordedAt,
                })
                .ToListAsync();

            return Results.Ok(new { data = records });
        });
    }
}