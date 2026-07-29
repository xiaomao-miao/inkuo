using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminUsageEndpoints
{
    public static void MapAdminUsageEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/usage").WithTags("usage").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db,
            int page = 1, int pageSize = 30,
            Guid? userId = null, Guid? modelId = null,
            DateTime? from = null, DateTime? to = null) =>
        {
            page = Math.Max(1, page);
            pageSize = Math.Clamp(pageSize, 1, 200);

            var q = db.UsageRecords
                .Include(u => u.User)
                .Include(u => u.ModelConfig)
                .Where(u => u.BillingStatus != "pending")
                .AsQueryable();

            if (userId.HasValue) q = q.Where(u => u.UserId == userId.Value);
            if (modelId.HasValue) q = q.Where(u => u.ModelConfigId == modelId.Value);
            if (from.HasValue) q = q.Where(u => u.RecordedAt >= from.Value);
            if (to.HasValue) q = q.Where(u => u.RecordedAt <= to.Value);

            var total = await q.CountAsync();
            var totalCost = await q.SumAsync(u => (long?)u.CostPoints) ?? 0L;
            var totalTokens = await q.SumAsync(u => (long?)u.PromptTokens + (long?)u.CompletionTokens) ?? 0L;

            var items = await q
                .OrderByDescending(u => u.RecordedAt)
                .Skip((page - 1) * pageSize)
                .Take(pageSize)
                .Select(u => new
                {
                    u.Id,
                    u.UserId,
                    UserEmail = u.User.Email,
                    u.ModelConfigId,
                    ModelName = u.ModelConfig.DisplayName,
                    u.PromptTokens,
                    u.CompletionTokens,
                    u.CostPoints,
                    u.BillingStatus,
                    u.RecordedAt,
                })
                .ToListAsync();

            return Results.Ok(new
            {
                total,
                page,
                pageSize,
                totalCostPoints = totalCost,
                totalTokens,
                items,
            });
        });
    }
}