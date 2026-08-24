namespace Inkuso.Cloud.Core.Entities;

public class ModelConfig
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string UpstreamProvider { get; set; } = string.Empty; // openai / deepseek; both use OpenAI-compatible chat completions
    public string UpstreamBaseUrl { get; set; } = string.Empty;
    public string UpstreamApiKey { get; set; } = string.Empty; // stored encrypted in production
    public string ModelName { get; set; } = string.Empty; // upstream model id, e.g. "deepseek-chat"
    public string DisplayName { get; set; } = string.Empty; // friendly name for UI
    public string? Description { get; set; }
    public decimal InputPricePerMTokens { get; set; } // yuan per 1M input tokens (uncached)
    public decimal OutputPricePerMTokens { get; set; } // yuan per 1M output tokens
    public decimal CachedInputPricePerMTokens { get; set; } // yuan per 1M cached input tokens (usually much cheaper, e.g. 0.1 of normal price)
    public bool Enabled { get; set; } = true;
    public int SortOrder { get; set; } = 0;
    public int MaxOutputTokens { get; set; } = 4096; // cap applied to max_tokens in the rewrite + used for reservation sizing
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
}
