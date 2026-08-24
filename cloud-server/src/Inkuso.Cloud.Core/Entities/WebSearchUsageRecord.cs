namespace Inkuso.Cloud.Core.Entities;

/// <summary>
/// One row per web_search call that routed through the cloud server.
/// Audit / abuse-tracing counterpart to <see cref="UsageRecord"/>; kept
/// as a separate table so existing chat-billing queries are not affected
/// (e.g. "sum of all chat tokens used this month" must not double-count
/// a web_search click).
/// </summary>
public class WebSearchUsageRecord
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public Guid UserId { get; set; }
    /// <summary>
    /// Provider id at the time of the call (e.g. "baike"). Foreign key to
    /// <see cref="WebSearchProvider.ProviderId"/>, kept as a string so a
    /// provider deletion doesn't orphan historical rows.
    /// </summary>
    public string ProviderId { get; set; } = string.Empty;
    public string Query { get; set; } = string.Empty;
    public long CostPoints { get; set; }
    public long? ReservedPoints { get; set; }
    public string? RequestId { get; set; }
    public string BillingStatus { get; set; } = "settled";
    public DateTime RecordedAt { get; set; } = DateTime.UtcNow;

    public User User { get; set; } = null!;
}
