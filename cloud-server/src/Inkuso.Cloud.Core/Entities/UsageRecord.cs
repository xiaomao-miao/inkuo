namespace Inkuso.Cloud.Core.Entities;

public class UsageRecord
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public Guid UserId { get; set; }
    public Guid ModelConfigId { get; set; }
    public long PromptTokens { get; set; }
    public long CachedPromptTokens { get; set; } // subset of PromptTokens that hit upstream cache
    public long CompletionTokens { get; set; }
    public decimal CostCents { get; set; } // calculated cost in cents
    public DateTime RecordedAt { get; set; } = DateTime.UtcNow;
    public string? RequestId { get; set; }

    public User User { get; set; } = null!;
    public ModelConfig ModelConfig { get; set; } = null!;
}
