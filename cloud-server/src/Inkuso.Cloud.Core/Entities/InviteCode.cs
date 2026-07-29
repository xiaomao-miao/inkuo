namespace Inkuso.Cloud.Core.Entities;

public class InviteCode
{
    public int Id { get; set; }
    public string Code { get; set; } = string.Empty;
    public long FreePoints { get; set; } = 1000; // e.g. 1000 = 1 元 free credit
    public int MaxUses { get; set; } = 1;
    public int UsedCount { get; set; } = 0;
    public DateTime? ExpiresAt { get; set; }
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public bool Enabled { get; set; } = true;
}
