namespace Inkuso.Cloud.Core.Entities;

public class RedemptionCode
{
    public int Id { get; set; }
    public string Code { get; set; } = string.Empty;
    public Guid? PlanId { get; set; } // null = credit only
    public decimal CreditCents { get; set; } // credit amount in cents
    public int MaxUses { get; set; } = 1;
    public int UsedCount { get; set; } = 0;
    public DateTime? ExpiresAt { get; set; }
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public bool Enabled { get; set; } = true;

    public Plan? Plan { get; set; }
}
