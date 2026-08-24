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
        int MaxOutputTokens,
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
        int MaxOutputTokens,
        bool Enabled,
        int SortOrder);

    public static void MapAdminModelConfigsEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/model-configs").WithTags("model-configs").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db) =>
        {
            var rows = await db.ModelConfigs
                .OrderBy(m => m.SortOrder)
                .ToListAsync();

            var items = rows.Select(m => new
            {
                m.Id,
                m.UpstreamProvider,
                m.UpstreamBaseUrl,
                HasUpstreamApiKey = !string.IsNullOrWhiteSpace(m.UpstreamApiKey),
                m.ModelName,
                m.DisplayName,
                m.Description,
                m.InputPricePerMTokens,
                m.OutputPricePerMTokens,
                m.CachedInputPricePerMTokens,
                m.MaxOutputTokens,
                m.Enabled,
                m.SortOrder,
                m.CreatedAt,
            });
            return Results.Ok(items);
        });

        group.MapPost("/", async (ModelConfigRequest req, AppDbContext db, ISecretProtector protector) =>
        {
            var endpointError = ValidateEndpoint(
                req.UpstreamProvider,
                req.UpstreamBaseUrl,
                req.DisplayName,
                req.ModelName);
            if (endpointError is not null)
                return Results.BadRequest(new { error = endpointError });
            var validationError = ValidateRateCard(
                req.InputPricePerMTokens,
                req.OutputPricePerMTokens,
                req.CachedInputPricePerMTokens,
                req.MaxOutputTokens);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });
            if (string.IsNullOrWhiteSpace(req.UpstreamApiKey))
                return Results.BadRequest(new { error = "UpstreamApiKey is required" });

            var m = new ModelConfig
            {
                UpstreamProvider = req.UpstreamProvider.Trim().ToLowerInvariant(),
                UpstreamBaseUrl = req.UpstreamBaseUrl.Trim().TrimEnd('/'),
                UpstreamApiKey = protector.Protect(req.UpstreamApiKey) ?? string.Empty,
                ModelName = req.ModelName.Trim(),
                DisplayName = req.DisplayName.Trim(),
                Description = req.Description?.Trim(),
                InputPricePerMTokens = req.InputPricePerMTokens,
                OutputPricePerMTokens = req.OutputPricePerMTokens,
                CachedInputPricePerMTokens = req.CachedInputPricePerMTokens,
                MaxOutputTokens = req.MaxOutputTokens,
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

            var endpointError = ValidateEndpoint(
                req.UpstreamProvider,
                req.UpstreamBaseUrl,
                req.DisplayName,
                req.ModelName);
            if (endpointError is not null)
                return Results.BadRequest(new { error = endpointError });
            var validationError = ValidateRateCard(
                req.InputPricePerMTokens,
                req.OutputPricePerMTokens,
                req.CachedInputPricePerMTokens,
                req.MaxOutputTokens);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });

            m.UpstreamProvider = req.UpstreamProvider.Trim().ToLowerInvariant();
            m.UpstreamBaseUrl = req.UpstreamBaseUrl.Trim().TrimEnd('/');
            if (!string.IsNullOrWhiteSpace(req.UpstreamApiKey))
                m.UpstreamApiKey = protector.Protect(req.UpstreamApiKey) ?? string.Empty; // leave existing if blank
            m.ModelName = req.ModelName.Trim();
            m.DisplayName = req.DisplayName.Trim();
            m.Description = req.Description?.Trim();
            m.InputPricePerMTokens = req.InputPricePerMTokens;
            m.OutputPricePerMTokens = req.OutputPricePerMTokens;
            m.CachedInputPricePerMTokens = req.CachedInputPricePerMTokens;
            m.MaxOutputTokens = req.MaxOutputTokens;
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

    private static string? ValidateRateCard(
        decimal inputPrice,
        decimal outputPrice,
        decimal cachedInputPrice,
        int maxOutputTokens)
    {
        const decimal maxStoredPrice = 999_999.999999m;
        if (inputPrice < 0 || outputPrice < 0 || cachedInputPrice < 0)
            return "Token prices cannot be negative";
        if (inputPrice > maxStoredPrice
            || outputPrice > maxStoredPrice
            || cachedInputPrice > maxStoredPrice)
            return $"Token prices must not exceed {maxStoredPrice}";
        if (maxOutputTokens is < 1 or > 131_072)
            return "MaxOutputTokens must be between 1 and 131072";
        return null;
    }

    private static string? ValidateEndpoint(
        string provider,
        string baseUrl,
        string displayName,
        string modelName)
    {
        if (string.IsNullOrWhiteSpace(displayName) || string.IsNullOrWhiteSpace(modelName))
            return "DisplayName and ModelName are required";

        var normalizedProvider = provider?.Trim().ToLowerInvariant();
        if (normalizedProvider is not ("openai" or "deepseek"))
            return "Only OpenAI-compatible providers are currently supported";

        if (!Uri.TryCreate(baseUrl?.Trim(), UriKind.Absolute, out var uri)
            || (uri.Scheme != Uri.UriSchemeHttps && uri.Scheme != Uri.UriSchemeHttp)
            || string.IsNullOrWhiteSpace(uri.Host))
            return "UpstreamBaseUrl must be an absolute http(s) URL";

        return null;
    }

}
