namespace Inkuso.Cloud.Core.Entities;

/// <summary>
/// Internal operator / admin user that can access the inkuo Cloud admin web UI.
/// Separate from <see cref="User"/> so that a customer user cannot log into the
/// admin panel even if they discover the endpoint.
/// </summary>
public class AdminUser
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Username { get; set; } = string.Empty;
    public string PasswordHash { get; set; } = string.Empty;
    public string Role { get; set; } = "admin"; // admin | superadmin
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public DateTime? LastLoginAt { get; set; }
    public bool Enabled { get; set; } = true;
}