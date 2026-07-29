using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class DashboardEndpoints
{
    public record DailyUsagePoint(DateTime Date, long CostPoints, long Tokens, int NewUsers);
    public record PlanDistribution(string PlanName, int Subscriptions);
    public record ModelUsageShare(string ModelName, long Tokens, long CostPoints);

    public static void MapDashboardEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/dashboard").WithTags("dashboard").RequireAuthorization();

        // Summary cards
        group.MapGet("/summary", async (AppDbContext db) =>
        {
            var now = DateTime.UtcNow;
            var monthStart = new DateTime(now.Year, now.Month, 1, 0, 0, 0, DateTimeKind.Utc);
            var dayStart = now.Date;

            var totalUsers = await db.Users.CountAsync();
            var newUsersThisMonth = await db.Users.CountAsync(u => u.CreatedAt >= monthStart);
            var newUsersToday = await db.Users.CountAsync(u => u.CreatedAt >= dayStart);

            var activeSubs = await db.Subscriptions.CountAsync(s => s.Status == "active");
            var totalInviteCodes = await db.InviteCodes.CountAsync();
            var usedInviteCodes = await db.InviteCodes.SumAsync(i => (int?)i.UsedCount) ?? 0;
            var totalRedemptionCodes = await db.RedemptionCodes.CountAsync();
            var usedRedemptionCodes = await db.RedemptionCodes.SumAsync(i => (int?)i.UsedCount) ?? 0;
            var suspendedUsers = await db.Users.CountAsync(u => u.IsSuspended);

            var usageQuery = db.UsageRecords.Where(u => u.BillingStatus != "pending");
            var monthCost = await usageQuery
                .Where(u => u.RecordedAt >= monthStart)
                .SumAsync(u => (long?)u.CostPoints) ?? 0L;
            var totalRevenue = await usageQuery.SumAsync(u => (long?)u.CostPoints) ?? 0L;
            var monthTokens = await usageQuery
                .Where(u => u.RecordedAt >= monthStart)
                .SumAsync(u => (long?)u.PromptTokens + (long?)u.CompletionTokens) ?? 0L;

            return Results.Ok(new
            {
                totalUsers,
                newUsersThisMonth,
                newUsersToday,
                activeSubscriptions = activeSubs,
                suspendedUsers,
                totalInviteCodes,
                usedInviteCodes,
                totalRedemptionCodes,
                usedRedemptionCodes,
                monthRevenuePoints = monthCost,
                totalRevenuePoints = totalRevenue,
                monthTokens,
            });
        });

        // Last 30 days usage time series
        group.MapGet("/usage-trend", async (AppDbContext db) =>
        {
            var start = DateTime.UtcNow.Date.AddDays(-29);

            var records = await db.UsageRecords
                .Where(u => u.RecordedAt >= start && u.BillingStatus != "pending")
                .GroupBy(u => u.RecordedAt.Date)
                .Select(g => new
                {
                    Date = g.Key,
                    Cost = g.Sum(u => u.CostPoints),
                    Tokens = g.Sum(u => u.PromptTokens + u.CompletionTokens),
                })
                .ToListAsync();

            var newUsers = await db.Users
                .Where(u => u.CreatedAt >= start)
                .GroupBy(u => u.CreatedAt.Date)
                .Select(g => new { Date = g.Key, Count = g.Count() })
                .ToListAsync();

            var lookupCost = records.ToDictionary(r => r.Date, r => (r.Cost, (long)r.Tokens));
            var lookupUsers = newUsers.ToDictionary(r => r.Date, r => r.Count);

            var series = new List<DailyUsagePoint>();
            for (int i = 0; i < 30; i++)
            {
                var d = start.AddDays(i);
                lookupCost.TryGetValue(d, out var ct);
                lookupUsers.TryGetValue(d, out var uc);
                series.Add(new DailyUsagePoint(d, ct.Cost, ct.Item2, uc));
            }
            return Results.Ok(series);
        });

        // Plan distribution
        group.MapGet("/plan-distribution", async (AppDbContext db) =>
        {
            var rows = await db.Subscriptions
                .Where(s => s.Status == "active")
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
                    !db.Subscriptions.Any(s => s.UserId == u.Id && s.Status == "active"))));

            return Results.Ok(result);
        });

        // Top models by usage in last 30 days
        group.MapGet("/model-usage", async (AppDbContext db) =>
        {
            var start = DateTime.UtcNow.Date.AddDays(-29);

            var rows = await db.UsageRecords
                .Where(u => u.RecordedAt >= start && u.BillingStatus != "pending")
                .GroupBy(u => u.ModelConfigId)
                .Select(g => new
                {
                    ModelConfigId = g.Key,
                    Tokens = g.Sum(u => (long)u.PromptTokens + u.CompletionTokens),
                    Cost = g.Sum(u => u.CostPoints),
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
}