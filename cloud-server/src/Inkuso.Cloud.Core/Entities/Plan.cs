namespace Inkuso.Cloud.Core.Entities;

public class Plan
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Name { get; set; } = string.Empty; // Free / Plus / Pro / Max
    public int MonthlyQuotaCents { get; set; } // monthly fee in cents (0 for free)
    public long MonthlyTokenLimit { get; set; } // token limit per month
    public decimal OverageInputPricePer1k { get; set; } // per 1k input tokens (yuan)
    public decimal OverageOutputPricePer1k { get; set; } // per 1k output tokens (yuan)
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public bool Enabled { get; set; } = true;
}