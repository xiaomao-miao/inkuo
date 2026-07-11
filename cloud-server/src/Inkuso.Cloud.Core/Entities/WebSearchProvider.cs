namespace Inkuso.Cloud.Core.Entities;

/// <summary>
/// Server-side web_search provider. Today only one provider is needed
/// (baidu baike for entity lookups) but the table is shaped so a future
/// "google / bing / tavily" addition is a one-row insert — no schema
/// change. The desktop client uses provider <c>id</c> as the dispatch
/// key, mirroring the convention used by the per-LLM model_configs.
/// </summary>
public class WebSearchProvider
{
    public Guid Id { get; set; } = Guid.NewGuid();
    /// <summary>
    /// Stable provider id used by the desktop-side dispatch (e.g. "baike").
    /// </summary>
    public string ProviderId { get; set; } = string.Empty;
    /// <summary>
    /// Friendly label for the admin UI (Chinese label is fine — admin
    /// users are operators and we surface it only there).
    /// </summary>
    public string DisplayName { get; set; } = string.Empty;
    /// <summary>
    /// Optional override of the upstream endpoint. <c>Null</c> means the
    /// compiled-in default URL (in <c>WebSearchForwarder</c>) is used;
    /// this lets operators point at a mirror without redeploying.
    /// </summary>
    public string? UpstreamBaseUrl { get; set; }
    /// <summary>
    /// Operator-supplied credential for the upstream API (Bearer token in
    /// Baidu AppBuilder's case; the field is intentionally generic so a
    /// future provider can store, say, a Google API key).
    /// </summary>
    public string? UpstreamApiKey { get; set; }
    /// <summary>
    /// Per-provider kill switch; when <c>false</c>, the desktop client
    /// sees a "this provider is currently disabled" error from the
    /// cloud server even when the master toggle is on.
    /// </summary>
    public bool Enabled { get; set; } = true;
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
}
