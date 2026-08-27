namespace Inkuso.Cloud.Core.Entities;

/// <summary>
/// A downloadable installer published by admins through the cloud admin UI.
/// Used by the marketing landing page (GET /api/releases) to show users
/// which versions are available, and by the admin panel to upload/manage
/// release artifacts.
/// </summary>
public class Release
{
    public Guid Id { get; set; } = Guid.NewGuid();

    /// <summary>Semantic version, e.g. "0.1.0".</summary>
    public string Version { get; set; } = string.Empty;

    /// <summary>Release channel: "stable" or "beta".</summary>
    public string Channel { get; set; } = "stable";

    /// <summary>Target OS platform. Today only "windows" is published.</summary>
    public string Platform { get; set; } = "windows";

    /// <summary>Target CPU architecture, e.g. "x86_64" / "aarch64".</summary>
    public string Architecture { get; set; } = "x86_64";

    /// <summary>Original file name as uploaded (for the download Content-Disposition).</summary>
    public string FileName { get; set; } = string.Empty;

    /// <summary>File size in bytes.</summary>
    public long FileSizeBytes { get; set; }

    /// <summary>Lowercase hex SHA-256 of the file contents.</summary>
    public string Sha256 { get; set; } = string.Empty;

    /// <summary>Absolute path on the admin server's local disk.</summary>
    public string StoragePath { get; set; } = string.Empty;

    /// <summary>Database-gated public download endpoint for this release.</summary>
    public string DownloadUrl { get; set; } = string.Empty;

    /// <summary>Markdown release notes shown on the landing page.</summary>
    public string? ReleaseNotes { get; set; }

    /// <summary>True if this is the recommended version on the landing page hero.</summary>
    public bool IsLatest { get; set; }

    /// <summary>False hides the release from public listings (and disables download).</summary>
    public bool Enabled { get; set; } = true;

    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;

    /// <summary>Optional admin who uploaded the release.</summary>
    public Guid? CreatedByAdminId { get; set; }
}
