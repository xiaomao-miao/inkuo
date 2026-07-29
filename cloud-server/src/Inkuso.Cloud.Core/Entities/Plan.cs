namespace Inkuso.Cloud.Core.Entities;

public class Plan
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Name { get; set; } = string.Empty; // Free / Plus / Pro / Max
    public long MonthlyPricePoints { get; set; } // monthly fee in points (0 for free)
    public long MonthlyTokenLimit { get; set; } // token limit per month
    public decimal OverageInputPricePer1k { get; set; } // per 1k input tokens (yuan). Internal cost is converted to points at 1 yuan = 1000 points.
    public decimal OverageOutputPricePer1k { get; set; } // per 1k output tokens (yuan). Internal cost is converted to points at 1 yuan = 1000 points.
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public bool Enabled { get; set; } = true;
}
