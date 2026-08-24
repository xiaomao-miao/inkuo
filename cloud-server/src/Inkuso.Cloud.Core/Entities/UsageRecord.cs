namespace Inkuso.Cloud.Core.Entities;

public class UsageRecord
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public Guid UserId { get; set; }
    public Guid ModelConfigId { get; set; }
    public long PromptTokens { get; set; }
    public long CachedPromptTokens { get; set; } // subset of PromptTokens that hit upstream cache
    public long CompletionTokens { get; set; }
    public long CostPoints { get; set; } // calculated cost in points (1 元 = 1000 点)
    public long? ReservedPoints { get; set; } // points reserved at request start; may be null for legacy/system records
    // Freeze the rate card when the hold is created. Nullable keeps legacy
    // records readable; settlement falls back to the current model price only
    // for reservations created before these columns existed.
    public decimal? InputPricePerMTokensSnapshot { get; set; }
    public decimal? OutputPricePerMTokensSnapshot { get; set; }
    public decimal? CachedInputPricePerMTokensSnapshot { get; set; }
    public DateTime RecordedAt { get; set; } = DateTime.UtcNow;
    public string? RequestId { get; set; }
    public string BillingStatus { get; set; } = "settled"; // pending | streaming | bill_pending | settled | released | debt

    public User User { get; set; } = null!;
    public ModelConfig ModelConfig { get; set; } = null!;
}
