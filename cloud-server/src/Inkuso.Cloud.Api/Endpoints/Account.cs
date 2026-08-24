using System.Security.Claims;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Account
{
    public record AccountInfo(Guid Id, string Email, long BalancePoints, long ReservedPoints, long DebtPoints,
        bool IsSuspended, string? PlanName, long MonthlyTokenLimit, DateTime? SubscriptionExpiresAt,
        long TokensUsedThisMonth, long MonthlyTokensRemaining);

    public record ChatUsageItem(
        Guid Id,
        string Model,
        long PromptTokens,
        long CompletionTokens,
        long CostPoints,
        string BillingStatus,
        DateTime RecordedAt);

    public record WebSearchUsageItem(
        Guid Id,
        string ProviderId,
        string Query,
        long CostPoints,
        long? ReservedPoints,
        string BillingStatus,
        DateTime RecordedAt);

    /// <summary>
    /// <see cref="Data"/> intentionally retains the original chat-only payload
    /// so existing desktop builds continue to deserialize it unchanged. Search
    /// billing is exposed as an additive section for newer clients.
    /// </summary>
    public record AccountUsageResponse(
        IReadOnlyList<ChatUsageItem> Data,
        IReadOnlyList<WebSearchUsageItem> WebSearchData);

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
                user.Id, user.Email, user.BalancePoints, user.ReservedPoints, user.DebtPoints, user.IsSuspended,
                sub?.Plan.Name, limit,
                sub?.ExpiresAt,
                tokensUsed, remaining
            ));
        });

        group.MapGet("/usage", async (HttpContext ctx, AppDbContext db, CancellationToken ct) =>
        {
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);
            return Results.Ok(await GetUsageAsync(userId, db, ct));
        });
    }

    public static async Task<AccountUsageResponse> GetUsageAsync(
        Guid userId,
        AppDbContext db,
        CancellationToken ct = default)
    {
        var chatRecords = await db.UsageRecords
            .Where(u => u.UserId == userId
                        && (u.BillingStatus == "settled"
                            || u.BillingStatus == "released"
                            || u.BillingStatus == "debt"
                            || u.BillingStatus == "estimated"))
            .OrderByDescending(u => u.RecordedAt)
            .Take(50)
            .Select(u => new ChatUsageItem(
                u.Id,
                u.ModelConfig.DisplayName,
                u.PromptTokens,
                u.CompletionTokens,
                u.CostPoints,
                u.BillingStatus,
                u.RecordedAt))
            .ToListAsync(ct);

        var webSearchRecords = await db.WebSearchUsageRecords
            .Where(u => u.UserId == userId
                        && (u.BillingStatus == "settled"
                            || u.BillingStatus == "released"))
            .OrderByDescending(u => u.RecordedAt)
            .Take(50)
            .Select(u => new WebSearchUsageItem(
                u.Id,
                u.ProviderId,
                u.Query,
                u.CostPoints,
                u.ReservedPoints,
                u.BillingStatus,
                u.RecordedAt))
            .ToListAsync(ct);

        return new AccountUsageResponse(chatRecords, webSearchRecords);
    }
}
