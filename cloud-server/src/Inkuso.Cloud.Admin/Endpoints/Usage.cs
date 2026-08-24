using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminUsageEndpoints
{
    public record AdminUsageItem(
        Guid Id,
        string UsageType,
        Guid UserId,
        string UserEmail,
        Guid? ModelConfigId,
        string? ModelName,
        string? ProviderId,
        string? Query,
        long PromptTokens,
        long CompletionTokens,
        long CostPoints,
        long? ReservedPoints,
        string BillingStatus,
        DateTime RecordedAt);

    public record AdminUsageResponse(
        int Total,
        int Page,
        int PageSize,
        long TotalCostPoints,
        long TotalTokens,
        int ChatRecords,
        int WebSearchRecords,
        long ChatCostPoints,
        long WebSearchCostPoints,
        IReadOnlyList<AdminUsageItem> Items);

    public static void MapAdminUsageEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/usage").WithTags("usage").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db,
            int page = 1, int pageSize = 30,
            Guid? userId = null, Guid? modelId = null,
            DateTime? from = null, DateTime? to = null,
            string? usageType = "chat",
            CancellationToken ct = default) =>
        {
            var response = await QueryUsageAsync(
                db, page, pageSize, userId, modelId, from, to, usageType, ct);
            return response is null
                ? Results.BadRequest(new { error = "usageType must be chat, search, or all" })
                : Results.Ok(response);
        });
    }

    /// <summary>
    /// The default remains chat-only for wire compatibility. The Admin SPA
    /// explicitly requests <c>all</c> and can filter to either durable ledger.
    /// </summary>
    public static async Task<AdminUsageResponse?> QueryUsageAsync(
        AppDbContext db,
        int page,
        int pageSize,
        Guid? userId = null,
        Guid? modelId = null,
        DateTime? from = null,
        DateTime? to = null,
        string? usageType = "chat",
        CancellationToken ct = default)
    {
        page = Math.Max(1, page);
        pageSize = Math.Clamp(pageSize, 1, 200);
        usageType = (usageType ?? "chat").Trim().ToLowerInvariant();
        if (usageType is not ("chat" or "search" or "all")) return null;

        var chatQuery = db.UsageRecords
            .Where(u => u.BillingStatus == "settled"
                        || u.BillingStatus == "released"
                        || u.BillingStatus == "debt"
                        || u.BillingStatus == "estimated");
        var searchQuery = db.WebSearchUsageRecords
            .Where(u => u.BillingStatus == "settled"
                        || u.BillingStatus == "released");

        if (userId.HasValue)
        {
            chatQuery = chatQuery.Where(u => u.UserId == userId.Value);
            searchQuery = searchQuery.Where(u => u.UserId == userId.Value);
        }
        if (modelId.HasValue)
        {
            chatQuery = chatQuery.Where(u => u.ModelConfigId == modelId.Value);
            // A model filter has no meaning for fixed-price web searches.
            searchQuery = searchQuery.Where(_ => false);
        }
        if (from.HasValue)
        {
            chatQuery = chatQuery.Where(u => u.RecordedAt >= from.Value);
            searchQuery = searchQuery.Where(u => u.RecordedAt >= from.Value);
        }
        if (to.HasValue)
        {
            chatQuery = chatQuery.Where(u => u.RecordedAt <= to.Value);
            searchQuery = searchQuery.Where(u => u.RecordedAt <= to.Value);
        }

        var chatRecords = usageType == "search" ? 0 : await chatQuery.CountAsync(ct);
        var searchRecords = usageType == "chat" ? 0 : await searchQuery.CountAsync(ct);
        var chatCost = usageType == "search"
            ? 0L
            : await chatQuery.SumAsync(u => (long?)u.CostPoints, ct) ?? 0L;
        var webSearchCost = usageType == "chat"
            ? 0L
            : await searchQuery.SumAsync(u => (long?)u.CostPoints, ct) ?? 0L;
        var totalTokens = usageType == "search"
            ? 0L
            : await chatQuery.SumAsync(
                u => (long?)u.PromptTokens + (long?)u.CompletionTokens, ct) ?? 0L;

        var chatItems = chatQuery.Select(u => new AdminUsageItem(
                u.Id,
                "chat",
                u.UserId,
                u.User.Email,
                u.ModelConfigId,
                u.ModelConfig.DisplayName,
                null,
                null,
                u.PromptTokens,
                u.CompletionTokens,
                u.CostPoints,
                u.ReservedPoints,
                u.BillingStatus,
                u.RecordedAt));
        var webSearchItems = searchQuery.Select(u => new AdminUsageItem(
                u.Id,
                "search",
                u.UserId,
                u.User.Email,
                null,
                null,
                u.ProviderId,
                u.Query,
                0L,
                0L,
                u.CostPoints,
                u.ReservedPoints,
                u.BillingStatus,
                u.RecordedAt));

        // Avoid a provider-sensitive UNION between text/varchar/null columns.
        // For the combined view, the global top K must be contained in the top
        // K of each source, so two bounded queries preserve exact pagination
        // without loading either full ledger into memory.
        var total = checked(chatRecords + searchRecords);
        var skipLong = (long)(page - 1) * pageSize;
        IReadOnlyList<AdminUsageItem> items;
        if (skipLong >= total)
        {
            items = [];
        }
        else
        {
            var skip = checked((int)skipLong);
            if (usageType == "chat")
            {
                items = await chatItems
                    .OrderByDescending(u => u.RecordedAt)
                    .ThenByDescending(u => u.Id)
                    .Skip(skip)
                    .Take(pageSize)
                    .ToListAsync(ct);
            }
            else if (usageType == "search")
            {
                items = await webSearchItems
                    .OrderByDescending(u => u.RecordedAt)
                    .ThenByDescending(u => u.Id)
                    .Skip(skip)
                    .Take(pageSize)
                    .ToListAsync(ct);
            }
            else
            {
                var candidateCount = checked((int)Math.Min(total, (long)skip + pageSize));
                var chatCandidates = await chatItems
                    .OrderByDescending(u => u.RecordedAt)
                    .ThenByDescending(u => u.Id)
                    .Take(candidateCount)
                    .ToListAsync(ct);
                var searchCandidates = await webSearchItems
                    .OrderByDescending(u => u.RecordedAt)
                    .ThenByDescending(u => u.Id)
                    .Take(candidateCount)
                    .ToListAsync(ct);
                items = chatCandidates
                    .Concat(searchCandidates)
                    .OrderByDescending(u => u.RecordedAt)
                    .ThenByDescending(u => u.Id)
                    .Skip(skip)
                    .Take(pageSize)
                    .ToList();
            }
        }

        return new AdminUsageResponse(
            total,
            page,
            pageSize,
            checked(chatCost + webSearchCost),
            totalTokens,
            chatRecords,
            searchRecords,
            chatCost,
            webSearchCost,
            items);
    }
}
