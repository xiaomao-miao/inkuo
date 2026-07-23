using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Security;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminModelConfigsEndpoints
{
    public record ModelConfigRequest(
        string UpstreamProvider,
        string UpstreamBaseUrl,
        string UpstreamApiKey,
        string ModelName,
        string DisplayName,
        string? Description,
        decimal InputPricePerMTokens,
        decimal OutputPricePerMTokens,
        decimal CachedInputPricePerMTokens,
        bool Enabled,
        int SortOrder);

    // Special request for update that allows leaving UpstreamApiKey blank (= keep existing)
    public record ModelConfigUpdateRequest(
        string UpstreamProvider,
        string UpstreamBaseUrl,
        string? UpstreamApiKey,
        string ModelName,
        string DisplayName,
        string? Description,
        decimal InputPricePerMTokens,
        decimal OutputPricePerMTokens,
        decimal CachedInputPricePerMTokens,
        bool Enabled,
        int SortOrder);

    public static void MapAdminModelConfigsEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/model-configs").WithTags("model-configs").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db, bool includeKey = false) =>
        {
            var rows = await db.ModelConfigs
                .OrderBy(m => m.SortOrder)
                .ToListAsync();

            var items = rows.Select(m => new
            {
                m.Id,
                m.UpstreamProvider,
                m.UpstreamBaseUrl,
                UpstreamApiKeyMasked = includeKey ? m.UpstreamApiKey : Mask(m.UpstreamApiKey),
                m.ModelName,
                m.DisplayName,
                m.Description,
                m.InputPricePerMTokens,
                m.OutputPricePerMTokens,
                m.CachedInputPricePerMTokens,
                m.Enabled,
                m.SortOrder,
                m.CreatedAt,
            });
            return Results.Ok(items);
        });

        group.MapPost("/", async (ModelConfigRequest req, AppDbContext db, ISecretProtector protector) =>
        {
            if (string.IsNullOrWhiteSpace(req.DisplayName) || string.IsNullOrWhiteSpace(req.ModelName))
                return Results.BadRequest(new { error = "DisplayName and ModelName are required" });
            if (string.IsNullOrWhiteSpace(req.UpstreamBaseUrl))
                return Results.BadRequest(new { error = "UpstreamBaseUrl is required" });

            var m = new ModelConfig
            {
                UpstreamProvider = req.UpstreamProvider,
                UpstreamBaseUrl = req.UpstreamBaseUrl.TrimEnd('/'),
                UpstreamApiKey = protector.Protect(req.UpstreamApiKey) ?? string.Empty,
                ModelName = req.ModelName,
                DisplayName = req.DisplayName,
                Description = req.Description,
                InputPricePerMTokens = req.InputPricePerMTokens,
                OutputPricePerMTokens = req.OutputPricePerMTokens,
                CachedInputPricePerMTokens = req.CachedInputPricePerMTokens,
                Enabled = req.Enabled,
                SortOrder = req.SortOrder,
                CreatedAt = DateTime.UtcNow,
            };
            db.ModelConfigs.Add(m);
            await db.SaveChangesAsync();
            return Results.Ok(new { id = m.Id });
        });

        group.MapPut("/{id:guid}", async (Guid id, ModelConfigUpdateRequest req, AppDbContext db, ISecretProtector protector) =>
        {
            var m = await db.ModelConfigs.FindAsync(id);
            if (m == null) return Results.NotFound();

            m.UpstreamProvider = req.UpstreamProvider;
            m.UpstreamBaseUrl = req.UpstreamBaseUrl.TrimEnd('/');
            if (!string.IsNullOrWhiteSpace(req.UpstreamApiKey))
                m.UpstreamApiKey = protector.Protect(req.UpstreamApiKey) ?? string.Empty; // leave existing if blank
            m.ModelName = req.ModelName;
            m.DisplayName = req.DisplayName;
            m.Description = req.Description;
            m.InputPricePerMTokens = req.InputPricePerMTokens;
            m.OutputPricePerMTokens = req.OutputPricePerMTokens;
            m.CachedInputPricePerMTokens = req.CachedInputPricePerMTokens;
            m.Enabled = req.Enabled;
            m.SortOrder = req.SortOrder;

            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Model config updated" });
        });

        group.MapDelete("/{id:guid}", async (Guid id, AppDbContext db) =>
        {
            var m = await db.ModelConfigs.FindAsync(id);
            if (m == null) return Results.NotFound();
            var hasUsage = await db.UsageRecords.AnyAsync(u => u.ModelConfigId == id);
            if (hasUsage)
                return Results.BadRequest(new { error = "Cannot delete model config with usage history; disable it instead" });
            db.ModelConfigs.Remove(m);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Model config deleted" });
        });
    }

    private static string Mask(string key)
    {
        if (string.IsNullOrEmpty(key)) return "";
        if (key.Length <= 8) return new string('*', key.Length);
        return key[..4] + "***" + key[^4..];
    }
}