namespace Inkuso.Cloud.Core.Entities;

public class RefreshToken
{
    public int Id { get; set; }
    public string Jti { get; set; } = Guid.NewGuid().ToString();
    public Guid UserId { get; set; }
    public DateTime ExpiresAt { get; set; }
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public bool Revoked { get; set; } = false;

    public User User { get; set; } = null!;
}
