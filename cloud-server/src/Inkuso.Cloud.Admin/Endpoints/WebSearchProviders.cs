// <copyright file="WebSearchProviders.cs" company="inkuo">
// Admin CRUD for `WebSearchProvider` rows. Mirrors the shape of
// `ModelConfigs.cs` so the admin UI can render it with the same
// Form/Table pattern: list with write-only key status, create /
// update where blank key means "keep existing", per-row enable toggle.
//
// All endpoints live under `/api/web-search-providers/` and require
// admin JWT (same scheme as the rest of the admin module).
// </copyright>

using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Security;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminWebSearchProvidersEndpoints
{
    public record WebSearchProviderRequest(
        string ProviderId,
        string DisplayName,
        string? UpstreamBaseUrl,
        string? UpstreamApiKey,
        bool Enabled);

    /// <summary>
    /// Update payload that lets the API key be omitted (= keep existing)
    /// without forcing the operator to re-paste it on every save.
    /// `ProviderId` is also optional — the row's ProviderId is fixed at
    /// create time (and the UI disables the field on edit), so blank /
    /// missing means "leave it as-is". This avoids 400s if the admin
    /// frontend's antd `disabled` input drops the value from the form
    /// submission.
    /// </summary>
    public record WebSearchProviderUpdateRequest(
        string? ProviderId,
        string? DisplayName,
        string? UpstreamBaseUrl,
        string? UpstreamApiKey,
        bool Enabled);

    public static void MapAdminWebSearchProvidersEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/web-search-providers").WithTags("web-search-providers").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db) =>
        {
            var rows = await db.WebSearchProviders
                .OrderBy(p => p.ProviderId)
                .ToListAsync();

            var items = rows.Select(p => new
            {
                p.Id,
                p.ProviderId,
                p.DisplayName,
                p.UpstreamBaseUrl,
                HasUpstreamApiKey = !string.IsNullOrWhiteSpace(p.UpstreamApiKey),
                p.Enabled,
                p.CreatedAt,
            });
            return Results.Ok(items);
        });

        group.MapPost("/", async (WebSearchProviderRequest req, AppDbContext db, ISecretProtector protector) =>
        {
            if (string.IsNullOrWhiteSpace(req.ProviderId))
                return Results.BadRequest(new { error = "ProviderId is required" });
            if (string.IsNullOrWhiteSpace(req.DisplayName))
                return Results.BadRequest(new { error = "DisplayName is required" });

            var pid = req.ProviderId.Trim().ToLowerInvariant();
            if (await db.WebSearchProviders.AnyAsync(p => p.ProviderId == pid))
                return Results.Conflict(new { error = $"Provider '{pid}' already exists" });

            var row = new WebSearchProvider
            {
                ProviderId = pid,
                DisplayName = req.DisplayName.Trim(),
                UpstreamBaseUrl = string.IsNullOrWhiteSpace(req.UpstreamBaseUrl)
                    ? null
                    : req.UpstreamBaseUrl.Trim(),
                UpstreamApiKey = string.IsNullOrWhiteSpace(req.UpstreamApiKey)
                    ? null
                    : protector.Protect(req.UpstreamApiKey.Trim()),
                Enabled = req.Enabled,
                CreatedAt = DateTime.UtcNow,
            };
            db.WebSearchProviders.Add(row);
            await db.SaveChangesAsync();
            return Results.Ok(new { id = row.Id });
        });

        group.MapPut("/{id:guid}", async (Guid id, WebSearchProviderUpdateRequest req, AppDbContext db, ISecretProtector protector, ILoggerFactory loggerFactory) =>
        {
            var logger = loggerFactory.CreateLogger("Admin.WebSearchProviders");
            var row = await db.WebSearchProviders.FindAsync(id);
            if (row is null) return Results.NotFound();

            // ProviderId rename is intentionally NOT exposed via PUT — the
            // ProviderId is a routing key referenced by usage records, and
            // the admin UI locks it in edit mode. If a caller sends it,
            // we ignore the field (rather than 400) so a stray form submit
            // doesn't fail with an opaque validation error.
            //
            // DisplayName and BaseUrl also follow the "blank = keep existing"
            // rule (mirrors the upstream API key convention). The frontend
            // validates both as required, so a 400 from this path almost
            // always means the form's `disabled`/submit quirk dropped the
            // field — keeping the existing row value is safer than rejecting
            // the operator's save.
            if (!string.IsNullOrWhiteSpace(req.DisplayName))
                row.DisplayName = req.DisplayName.Trim();
            row.UpstreamBaseUrl = string.IsNullOrWhiteSpace(req.UpstreamBaseUrl)
                ? row.UpstreamBaseUrl
                : req.UpstreamBaseUrl.Trim();
            // Blank key on update = "keep existing"; we don't want a
            // naked save-without-typing to wipe the operator's key.
            if (!string.IsNullOrWhiteSpace(req.UpstreamApiKey))
                row.UpstreamApiKey = protector.Protect(req.UpstreamApiKey.Trim());
            row.Enabled = req.Enabled;

            logger.LogInformation(
                "WebSearchProvider PUT id={Id} sent DisplayName={DisplayName} BaseUrl={BaseUrl} HasKey={HasKey} Enabled={Enabled}; row -> DisplayName={FinalDisplayName}",
                id, req.DisplayName, req.UpstreamBaseUrl, !string.IsNullOrWhiteSpace(req.UpstreamApiKey), req.Enabled, row.DisplayName);

            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateConcurrencyException)
            {
                return Results.Conflict(new { error = "Concurrent update, please retry" });
            }
            return Results.Ok(new { message = "Web search provider updated" });
        });

        group.MapDelete("/{id:guid}", async (Guid id, AppDbContext db) =>
        {
            var row = await db.WebSearchProviders.FindAsync(id);
            if (row is null) return Results.NotFound();

            var hasUsage = await db.WebSearchUsageRecords.AnyAsync(u => u.ProviderId == row.ProviderId);
            if (hasUsage)
            {
                // Disable rather than delete so historical usage rows
                // can still reference a ProviderId. The desktop client
                // will see this provider as `disabled` and surface the
                // usual "provider disabled" error.
                row.Enabled = false;
                await db.SaveChangesAsync();
                return Results.Ok(new { message = "Web search provider disabled (had usage history)" });
            }

            db.WebSearchProviders.Remove(row);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Web search provider deleted" });
        });
    }

}
