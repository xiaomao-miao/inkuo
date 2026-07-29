using System.Security.Claims;
using System.Security.Cryptography;
using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Admin.Endpoints;

/// <summary>
/// Manages downloadable installer releases. Two endpoint groups share
/// <c>/api/releases</c>:
///   - public (no auth): listing the currently enabled releases for the
///     marketing landing page, plus a single-item lookup. The actual
///     installer binary is served as a static file under
///     <c>/releases-files/{stored-name}</c> so users can fetch it without
///     a JWT.
///   - admin (Bearer): upload a new release, toggle enabled / isLatest,
///     and delete (which removes the file from disk).
/// </summary>
public static class AdminReleasesEndpoints
{
    public const string StaticRequestPath = "/releases-files";

    public record ToggleEnabledRequest(bool Enabled);
    public record ToggleLatestRequest(bool IsLatest);

    public static void MapAdminReleasesEndpoints(this WebApplication app)
    {
        // ---------- Public (no auth) ----------
        var publicGroup = app.MapGroup("/api/releases").WithTags("releases-public").AllowAnonymous();

        publicGroup.MapGet("/", async (AppDbContext db) =>
        {
            var rows = await db.Releases
                .Where(r => r.Enabled)
                .OrderByDescending(r => r.IsLatest)
                .ThenByDescending(r => r.CreatedAt)
                .Select(r => new
                {
                    r.Id,
                    r.Version,
                    r.Channel,
                    r.Platform,
                    r.Architecture,
                    r.FileName,
                    r.FileSizeBytes,
                    r.Sha256,
                    r.DownloadUrl,
                    r.ReleaseNotes,
                    r.IsLatest,
                    r.CreatedAt,
                })
                .ToListAsync();
            return Results.Ok(rows);
        });

        publicGroup.MapGet("/latest", async (AppDbContext db) =>
        {
            var latest = await db.Releases
                .Where(r => r.Enabled && r.IsLatest)
                .OrderByDescending(r => r.CreatedAt)
                .Select(r => new
                {
                    r.Id,
                    r.Version,
                    r.Channel,
                    r.Platform,
                    r.Architecture,
                    r.FileName,
                    r.FileSizeBytes,
                    r.Sha256,
                    r.DownloadUrl,
                    r.ReleaseNotes,
                    r.CreatedAt,
                })
                .FirstOrDefaultAsync();
            return latest == null ? Results.NotFound() : Results.Ok(latest);
        });

        // ---------- Admin (Bearer) ----------
        var adminGroup = app.MapGroup("/api/releases").WithTags("releases-admin").RequireAuthorization();

        adminGroup.MapGet("/admin/all", async (AppDbContext db) =>
        {
            var rows = await db.Releases
                .OrderByDescending(r => r.CreatedAt)
                .Select(r => new
                {
                    r.Id,
                    r.Version,
                    r.Channel,
                    r.Platform,
                    r.Architecture,
                    r.FileName,
                    r.FileSizeBytes,
                    r.Sha256,
                    r.DownloadUrl,
                    r.StoragePath,
                    r.ReleaseNotes,
                    r.IsLatest,
                    r.Enabled,
                    r.CreatedAt,
                    r.CreatedByAdminId,
                })
                .ToListAsync();
            return Results.Ok(rows);
        });

        // Multipart upload using the minimal-API's built-in IFormFile binding —
        // a much more reliable path than manually calling req.ReadFormAsync()
        // and then file.OpenReadStream(), which can deadlock under load.
        // The 2 GiB limit is configured globally via Kestrel + FormOptions
        // in Program.cs.
        adminGroup.MapPost("/upload", async (
            [Microsoft.AspNetCore.Mvc.FromForm] IFormFile? file,
            [Microsoft.AspNetCore.Mvc.FromForm] string? version,
            [Microsoft.AspNetCore.Mvc.FromForm] string? channel,
            [Microsoft.AspNetCore.Mvc.FromForm] string? platform,
            [Microsoft.AspNetCore.Mvc.FromForm] string? architecture,
            [Microsoft.AspNetCore.Mvc.FromForm] string? releaseNotes,
            [Microsoft.AspNetCore.Mvc.FromForm] string? isLatest,
            [Microsoft.AspNetCore.Mvc.FromForm] string? enabled,
            HttpContext httpCtx,
            AppDbContext db,
            IConfiguration cfg,
            ILogger<Program> logger) =>
        {
            if (file == null || file.Length == 0)
                return Results.BadRequest(new { error = "file is required" });

            version = version?.Trim();
            if (string.IsNullOrWhiteSpace(version))
                return Results.BadRequest(new { error = "version is required" });

            if (string.IsNullOrWhiteSpace(channel)) channel = "stable";
            if (string.IsNullOrWhiteSpace(platform)) platform = "windows";
            if (string.IsNullOrWhiteSpace(architecture)) architecture = "x86_64";

            var releaseNotesText = string.IsNullOrWhiteSpace(releaseNotes) ? null : releaseNotes.Trim();
            var isLatestFlag = string.Equals(isLatest, "true", StringComparison.OrdinalIgnoreCase);
            var enabledFlag = !string.Equals(enabled, "false", StringComparison.OrdinalIgnoreCase);

            // Only one platform for now.
            if (!string.Equals(platform, "windows", StringComparison.OrdinalIgnoreCase))
                return Results.BadRequest(new { error = $"platform '{platform}' is not yet supported" });

            // Refuse duplicates.
            var exists = await db.Releases.AnyAsync(r =>
                r.Platform == platform && r.Architecture == architecture
                && r.Channel == channel && r.Version == version);
            if (exists)
                return Results.Conflict(new { error = "A release with this version already exists" });

            // Refuse oversized uploads (default 2 GiB cap).
            const long maxBytes = 2L * 1024 * 1024 * 1024;
            if (file.Length > maxBytes)
                return Results.BadRequest(new { error = $"file exceeds {maxBytes / (1024 * 1024)} MiB cap" });

            var storageDir = cfg["Releases:StorageDir"] ?? "/var/lib/inkuo/releases";
            Directory.CreateDirectory(storageDir);

            var id = Guid.NewGuid();
            var ext = Path.GetExtension(file.FileName);
            if (string.IsNullOrWhiteSpace(ext)) ext = ".bin";
            if (ext.Length > 16) ext = ".bin";
            var storedName = id.ToString("N") + ext;
            var storagePath = Path.Combine(storageDir, storedName);

            string sha256;
            long bytesWritten;
            try
            {
                await using (var fs = new FileStream(storagePath, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                {
                    using var sha = SHA256.Create();
                    var buf = new byte[81920];
                    long total = 0;
                    await using var src = file.OpenReadStream();
                    int read;
                    while ((read = await src.ReadAsync(buf, 0, buf.Length)) > 0)
                    {
                        sha.TransformBlock(buf, 0, read, null, 0);
                        await fs.WriteAsync(buf, 0, read);
                        total += read;
                    }
                    sha.TransformFinalBlock(Array.Empty<byte>(), 0, 0);
                    bytesWritten = total;
                    sha256 = Convert.ToHexString(sha.Hash ?? Array.Empty<byte>()).ToLowerInvariant();
                }
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "Failed to persist uploaded release {Version}", version);
                try { if (File.Exists(storagePath)) File.Delete(storagePath); } catch { /* best effort */ }
                return Results.Problem(detail: "Failed to save uploaded file", statusCode: 500);
            }

            var downloadUrl = $"{StaticRequestPath}/{storedName}";

            var adminIdStr = httpCtx.User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                             ?? httpCtx.User.FindFirst("sub")?.Value;
            Guid? createdByAdminId = Guid.TryParse(adminIdStr, out var g) ? g : null;

            var release = new Release
            {
                Id = id,
                Version = version!,
                Channel = channel!,
                Platform = platform!,
                Architecture = architecture!,
                FileName = Path.GetFileName(file.FileName),
                FileSizeBytes = bytesWritten,
                Sha256 = sha256,
                StoragePath = storagePath,
                DownloadUrl = downloadUrl,
                ReleaseNotes = releaseNotesText,
                IsLatest = isLatestFlag,
                Enabled = enabledFlag,
                CreatedAt = DateTime.UtcNow,
                CreatedByAdminId = createdByAdminId,
            };

            // If we marked this release latest, demote any prior latest for the
            // same platform/architecture/channel.
            if (isLatestFlag)
            {
                var prior = await db.Releases
                    .Where(r => r.IsLatest && r.Platform == platform
                                && r.Architecture == architecture && r.Channel == channel)
                    .ToListAsync();
                foreach (var p in prior) p.IsLatest = false;
            }

            db.Releases.Add(release);
            await db.SaveChangesAsync();

            return Results.Ok(new
            {
                release.Id,
                release.Version,
                release.FileName,
                release.FileSizeBytes,
                release.Sha256,
                release.DownloadUrl,
                release.CreatedAt,
            });
        }).DisableAntiforgery();

        adminGroup.MapPatch("/{id:guid}/enabled", async (Guid id, ToggleEnabledRequest req, AppDbContext db) =>
        {
            var r = await db.Releases.FindAsync(id);
            if (r == null) return Results.NotFound();
            r.Enabled = req.Enabled;
            await db.SaveChangesAsync();
            return Results.Ok(new { id, enabled = r.Enabled });
        });

        adminGroup.MapPatch("/{id:guid}/latest", async (Guid id, ToggleLatestRequest req, AppDbContext db) =>
        {
            var r = await db.Releases.FindAsync(id);
            if (r == null) return Results.NotFound();

            if (req.IsLatest)
            {
                // Demote any other latest in the same platform/architecture/channel.
                var others = await db.Releases
                    .Where(x => x.IsLatest && x.Id != id
                                && x.Platform == r.Platform
                                && x.Architecture == r.Architecture
                                && x.Channel == r.Channel)
                    .ToListAsync();
                foreach (var o in others) o.IsLatest = false;
            }
            r.IsLatest = req.IsLatest;
            await db.SaveChangesAsync();
            return Results.Ok(new { id, isLatest = r.IsLatest });
        });

        adminGroup.MapDelete("/{id:guid}", async (Guid id, AppDbContext db) =>
        {
            var r = await db.Releases.FindAsync(id);
            if (r == null) return Results.NotFound();

            // Remove the file from disk (best-effort; ignore if already gone).
            try { if (File.Exists(r.StoragePath)) File.Delete(r.StoragePath); }
            catch { /* ignore — DB row is removed regardless */ }

            db.Releases.Remove(r);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Release deleted", id });
        });
    }
}