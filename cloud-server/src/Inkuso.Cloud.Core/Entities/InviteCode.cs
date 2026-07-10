namespace Inkuso.Cloud.Core.Entities;

public class InviteCode
{
    public int Id { get; set; }
    public string Code { get; set; } = string.Empty;
    public decimal FreeQuotaCents { get; set; } = 100; // e.g. 100 = 1 yuan free credit
    public int MaxUses { get; set; } = 1;
    public int UsedCount { get; set; } = 0;
    public DateTime? ExpiresAt { get; set; }
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public bool Enabled { get; set; } = true;
}
