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
///     installer binary is resolved through a database-backed public endpoint
///     so disabling a release also revokes its existing URL.
///   - admin (Bearer): upload a new release, toggle enabled / isLatest,
///     and delete (which removes the file from disk).
/// </summary>
public static class AdminReleasesEndpoints
{
    public const long MaxUploadBytes = 2L * 1024 * 1024 * 1024;
    private const long MaxUploadRequestBytes = MaxUploadBytes + 2L * 1024 * 1024;

    public record ToggleEnabledRequest(bool Enabled);
    public record ToggleLatestRequest(bool IsLatest);

    public static void MapAdminReleasesEndpoints(this WebApplication app)
    {
        // ---------- Public (no auth) ----------
        var publicGroup = app.MapGroup("/api/releases").WithTags("releases-public").AllowAnonymous();

        publicGroup.MapGet("/", async (HttpContext context, AppDbContext db) =>
        {
            context.Response.Headers.CacheControl = "no-store";
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
                    DownloadUrl = $"/api/releases/{r.Id}/download",
                    r.ReleaseNotes,
                    r.IsLatest,
                    r.CreatedAt,
                })
                .ToListAsync();
            return Results.Ok(rows);
        });

        publicGroup.MapGet("/latest", async (HttpContext context, AppDbContext db) =>
        {
            context.Response.Headers.CacheControl = "no-store";
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
                    DownloadUrl = $"/api/releases/{r.Id}/download",
                    r.ReleaseNotes,
                    r.CreatedAt,
                })
                .FirstOrDefaultAsync();
            return latest == null ? Results.NotFound() : Results.Ok(latest);
        });

        // Resolve downloads through the database instead of exposing the
        // storage directory as static files. This makes `Enabled = false`
        // actually revoke an existing URL and lets us provide a safe original
        // filename while still storing artifacts under unguessable names.
        publicGroup.MapGet("/{id:guid}/download", async (Guid id, HttpContext context, AppDbContext db) =>
        {
            context.Response.Headers.CacheControl = "no-store";
            var release = await db.Releases.AsNoTracking()
                .FirstOrDefaultAsync(r => r.Id == id && r.Enabled);
            if (release is null || !File.Exists(release.StoragePath))
                return Results.NotFound();

            // Browsers and intermediate proxies must revalidate the database
            // gate on every request; otherwise a previously cached installer
            // could remain downloadable after an admin disables it.
            return Results.File(
                release.StoragePath,
                contentType: "application/octet-stream",
                fileDownloadName: release.FileName,
                enableRangeProcessing: true);
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
                    DownloadUrl = $"/api/releases/{r.Id}/download",
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
        // Request-size metadata below raises the limit only for this
        // authenticated endpoint; every public/admin JSON route keeps
        // Kestrel's conservative default.
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
            if (version.Length > 64)
                return Results.BadRequest(new { error = "version must be at most 64 characters" });

            channel = string.IsNullOrWhiteSpace(channel) ? "stable" : channel.Trim().ToLowerInvariant();
            platform = string.IsNullOrWhiteSpace(platform) ? "windows" : platform.Trim().ToLowerInvariant();
            architecture = string.IsNullOrWhiteSpace(architecture) ? "x86_64" : architecture.Trim().ToLowerInvariant();
            if (channel is not ("stable" or "beta"))
                return Results.BadRequest(new { error = "channel must be stable or beta" });
            if (architecture is not ("x86_64" or "aarch64"))
                return Results.BadRequest(new { error = "architecture must be x86_64 or aarch64" });

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

            if (file.Length > MaxUploadBytes)
                return Results.BadRequest(new { error = $"file exceeds {MaxUploadBytes / (1024 * 1024)} MiB cap" });

            var storageDir = cfg["Releases:StorageDir"] ?? "/var/lib/inkuo/releases";
            Directory.CreateDirectory(storageDir);

            var id = Guid.NewGuid();
            // Browsers may send either slash convention in Content-Disposition.
            // Store only a leaf name and reject control characters before the
            // value is echoed into a future download response header.
            var originalFileName = Path.GetFileName(file.FileName.Replace('\\', '/'));
            if (string.IsNullOrWhiteSpace(originalFileName)
                || originalFileName.Length > 256
                || originalFileName.Any(char.IsControl))
                return Results.BadRequest(new { error = "file name must be between 1 and 256 characters" });
            var ext = Path.GetExtension(originalFileName).ToLowerInvariant();
            if (ext is not (".exe" or ".msi" or ".msix" or ".zip"))
                return Results.BadRequest(new { error = "unsupported release file type" });
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

            var downloadUrl = $"/api/releases/{id}/download";

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
                FileName = originalFileName,
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
            try
            {
                await db.SaveChangesAsync();
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "Failed to create release row for {Version}", version);
                try { File.Delete(storagePath); } catch { /* best effort */ }
                return Results.Problem(detail: "Failed to publish release", statusCode: 500);
            }

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
        })
        .DisableAntiforgery()
        .WithMetadata(
            // Multipart framing adds a small amount beyond the file itself.
            // Keep the artifact cap at exactly 2 GiB while allowing that
            // bounded protocol overhead through the request parser.
            new Microsoft.AspNetCore.Mvc.RequestSizeLimitAttribute(MaxUploadRequestBytes),
            new Microsoft.AspNetCore.Mvc.RequestFormLimitsAttribute
            {
                MultipartBodyLengthLimit = MaxUploadRequestBytes,
                ValueLengthLimit = 1024 * 1024,
            });

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

            var storagePath = r.StoragePath;
            db.Releases.Remove(r);
            await db.SaveChangesAsync();
            // Delete only after the row is committed. If filesystem cleanup
            // fails, the orphan is no longer reachable through the download
            // endpoint; deleting first could leave a live row with no file.
            try { if (File.Exists(storagePath)) File.Delete(storagePath); }
            catch { /* best-effort orphan cleanup */ }
            return Results.Ok(new { message = "Release deleted", id });
        });
    }
}
