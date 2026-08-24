using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class DashboardEndpoints
{
    public record DashboardSummary(
        int TotalUsers,
        int NewUsersThisMonth,
        int NewUsersToday,
        int ActiveSubscriptions,
        int SuspendedUsers,
        int TotalInviteCodes,
        int UsedInviteCodes,
        int TotalRedemptionCodes,
        int UsedRedemptionCodes,
        long MonthRevenuePoints,
        long TotalRevenuePoints,
        long MonthChatRevenuePoints,
        long TotalChatRevenuePoints,
        long MonthWebSearchRevenuePoints,
        long TotalWebSearchRevenuePoints,
        int MonthWebSearchRequests,
        int TotalWebSearchRequests,
        long MonthTokens);

    public record DailyUsagePoint(
        DateTime Date,
        long CostPoints,
        long Tokens,
        int NewUsers,
        long ChatCostPoints,
        long WebSearchCostPoints,
        int ChatRequests,
        int WebSearchRequests);
    public record PlanDistribution(string PlanName, int Subscriptions);
    public record ModelUsageShare(string ModelName, long Tokens, long CostPoints);

    public static void MapDashboardEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/dashboard").WithTags("dashboard").RequireAuthorization();

        // Summary cards
        group.MapGet("/summary", async (AppDbContext db, CancellationToken ct) =>
            Results.Ok(await GetSummaryAsync(db, DateTime.UtcNow, ct)));

        // Last 30 days usage time series
        group.MapGet("/usage-trend", async (AppDbContext db, CancellationToken ct) =>
            Results.Ok(await GetUsageTrendAsync(db, DateTime.UtcNow, ct)));

        // Plan distribution
        group.MapGet("/plan-distribution", async (AppDbContext db) =>
        {
            var now = DateTime.UtcNow;
            var rows = await db.Subscriptions
                .Where(s => s.Status == "active" && s.ExpiresAt > now)
                .GroupBy(s => s.PlanId)
                .Select(g => new { PlanId = g.Key, Count = g.Count() })
                .ToListAsync();

            var plans = await db.Plans.ToDictionaryAsync(p => p.Id, p => p.Name);
            var result = rows.Select(r => new PlanDistribution(
                plans.TryGetValue(r.PlanId, out var n) ? n : "Unknown",
                r.Count));

            // Always include Free even if no subs
            if (!result.Any(p => p.PlanName == "Free"))
                result = result.Append(new PlanDistribution("Free", await db.Users.CountAsync(u =>
                    !db.Subscriptions.Any(s => s.UserId == u.Id
                                               && s.Status == "active"
                                               && s.ExpiresAt > now))));

            return Results.Ok(result);
        });

        // Top models by usage in last 30 days
        group.MapGet("/model-usage", async (AppDbContext db) =>
        {
            var start = DateTime.UtcNow.Date.AddDays(-29);

            var rows = await db.UsageRecords
                .Where(u => u.RecordedAt >= start
                            && (u.BillingStatus == "settled"
                                || u.BillingStatus == "estimated"
                                || u.BillingStatus == "debt"))
                .GroupBy(u => u.ModelConfigId)
                .Select(g => new
                {
                    ModelConfigId = g.Key,
                    Tokens = g.Sum(u => (long)u.PromptTokens + u.CompletionTokens),
                    Cost = g.Sum(u => u.BillingStatus == "debt"
                        ? u.ReservedPoints ?? 0L
                        : u.CostPoints),
                })
                .OrderByDescending(r => r.Tokens)
                .Take(20)
                .ToListAsync();

            var modelIds = rows.Select(r => r.ModelConfigId).ToList();
            var models = await db.ModelConfigs
                .Where(m => modelIds.Contains(m.Id))
                .ToDictionaryAsync(m => m.Id, m => m.DisplayName);

            var result = rows.Select(r => new ModelUsageShare(
                models.TryGetValue(r.ModelConfigId, out var n) ? n : "Unknown",
                r.Tokens,
                r.Cost));

            return Results.Ok(result);
        });
    }

    public static async Task<DashboardSummary> GetSummaryAsync(
        AppDbContext db,
        DateTime now,
        CancellationToken ct = default)
    {
        var monthStart = new DateTime(now.Year, now.Month, 1, 0, 0, 0, DateTimeKind.Utc);
        var dayStart = now.Date;

        var totalUsers = await db.Users.CountAsync(ct);
        var newUsersThisMonth = await db.Users.CountAsync(u => u.CreatedAt >= monthStart, ct);
        var newUsersToday = await db.Users.CountAsync(u => u.CreatedAt >= dayStart, ct);
        var activeSubs = await db.Subscriptions.CountAsync(
            s => s.Status == "active" && s.ExpiresAt > now, ct);
        var totalInviteCodes = await db.InviteCodes.CountAsync(ct);
        var usedInviteCodes = await db.InviteCodes.CountAsync(i => i.UsedCount > 0, ct);
        var totalRedemptionCodes = await db.RedemptionCodes.CountAsync(ct);
        var usedRedemptionCodes = await db.RedemptionCodes.CountAsync(i => i.UsedCount > 0, ct);
        var suspendedUsers = await db.Users.CountAsync(u => u.IsSuspended, ct);

        // Chat debt is only revenue to the extent the original hold was
        // collected. Search revenue is deliberately stricter: the fixed-price
        // search ledger contributes only once it reaches `settled`.
        var chatUsage = db.UsageRecords.Where(u =>
            u.BillingStatus == "settled"
            || u.BillingStatus == "estimated"
            || u.BillingStatus == "debt");
        var monthChatRevenue = await chatUsage
            .Where(u => u.RecordedAt >= monthStart)
            .SumAsync(u => (long?)(u.BillingStatus == "debt"
                ? u.ReservedPoints ?? 0L
                : u.CostPoints), ct) ?? 0L;
        var totalChatRevenue = await chatUsage
            .SumAsync(u => (long?)(u.BillingStatus == "debt"
                ? u.ReservedPoints ?? 0L
                : u.CostPoints), ct) ?? 0L;
        var monthTokens = await chatUsage
            .Where(u => u.RecordedAt >= monthStart)
            .SumAsync(u => (long?)u.PromptTokens + (long?)u.CompletionTokens, ct) ?? 0L;

        var settledSearches = db.WebSearchUsageRecords
            .Where(u => u.BillingStatus == "settled");
        var monthWebSearchRevenue = await settledSearches
            .Where(u => u.RecordedAt >= monthStart)
            .SumAsync(u => (long?)u.CostPoints, ct) ?? 0L;
        var totalWebSearchRevenue = await settledSearches
            .SumAsync(u => (long?)u.CostPoints, ct) ?? 0L;
        var monthWebSearchRequests = await settledSearches
            .CountAsync(u => u.RecordedAt >= monthStart, ct);
        var totalWebSearchRequests = await settledSearches.CountAsync(ct);

        return new DashboardSummary(
            totalUsers,
            newUsersThisMonth,
            newUsersToday,
            activeSubs,
            suspendedUsers,
            totalInviteCodes,
            usedInviteCodes,
            totalRedemptionCodes,
            usedRedemptionCodes,
            checked(monthChatRevenue + monthWebSearchRevenue),
            checked(totalChatRevenue + totalWebSearchRevenue),
            monthChatRevenue,
            totalChatRevenue,
            monthWebSearchRevenue,
            totalWebSearchRevenue,
            monthWebSearchRequests,
            totalWebSearchRequests,
            monthTokens);
    }

    public static async Task<IReadOnlyList<DailyUsagePoint>> GetUsageTrendAsync(
        AppDbContext db,
        DateTime now,
        CancellationToken ct = default)
    {
        var start = now.Date.AddDays(-29);

        var chatRecords = await db.UsageRecords
            .Where(u => u.RecordedAt >= start
                        && (u.BillingStatus == "settled"
                            || u.BillingStatus == "estimated"
                            || u.BillingStatus == "debt"))
            .GroupBy(u => u.RecordedAt.Date)
            .Select(g => new
            {
                Date = g.Key,
                Cost = g.Sum(u => u.BillingStatus == "debt"
                    ? u.ReservedPoints ?? 0L
                    : u.CostPoints),
                Tokens = g.Sum(u => u.PromptTokens + u.CompletionTokens),
                Requests = g.Count(),
            })
            .ToListAsync(ct);

        var webSearchRecords = await db.WebSearchUsageRecords
            .Where(u => u.RecordedAt >= start && u.BillingStatus == "settled")
            .GroupBy(u => u.RecordedAt.Date)
            .Select(g => new
            {
                Date = g.Key,
                Cost = g.Sum(u => u.CostPoints),
                Requests = g.Count(),
            })
            .ToListAsync(ct);

        var newUsers = await db.Users
            .Where(u => u.CreatedAt >= start)
            .GroupBy(u => u.CreatedAt.Date)
            .Select(g => new { Date = g.Key, Count = g.Count() })
            .ToListAsync(ct);

        var chatByDate = chatRecords.ToDictionary(
            r => r.Date,
            r => (Cost: r.Cost, Tokens: (long)r.Tokens, Requests: r.Requests));
        var searchByDate = webSearchRecords.ToDictionary(
            r => r.Date,
            r => (Cost: r.Cost, Requests: r.Requests));
        var usersByDate = newUsers.ToDictionary(r => r.Date, r => r.Count);

        var series = new List<DailyUsagePoint>(30);
        for (var i = 0; i < 30; i++)
        {
            var date = start.AddDays(i);
            chatByDate.TryGetValue(date, out var chat);
            searchByDate.TryGetValue(date, out var search);
            usersByDate.TryGetValue(date, out var userCount);
            series.Add(new DailyUsagePoint(
                date,
                checked(chat.Cost + search.Cost),
                chat.Tokens,
                userCount,
                chat.Cost,
                search.Cost,
                chat.Requests,
                search.Requests));
        }

        return series;
    }
}
