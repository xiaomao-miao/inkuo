// <copyright file="WebSearchForwarder.cs" company="inkuo">
// Server-side web_search forwarder. Mirrors the role of LlmForwarder but
// for the web_search tool: it holds the operator-supplied upstream key
// (and optional base-url override) and issues the actual outbound HTTP
// call on behalf of every cloud-authenticated user.
//
// Today only the Baidu Baike provider is implemented. The dispatcher is
// data-driven via `provider_id` so a future "google / bing / tavily"
// addition only needs to extend `ExecuteAsync`'s switch — no schema or
// endpoint shape change.
// </copyright>

using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Security;

namespace Inkuso.Cloud.Core.Upstream;

public class WebSearchForwarder
{
    private readonly IHttpClientFactory _httpFactory;
    private readonly IServiceScopeFactory _scopeFactory;
    private readonly ILogger<WebSearchForwarder> _logger;
    private readonly ISecretProtector _secrets;

    /// <summary>
    /// Hard ceiling on the inbound `max_results` arg. Defends against
    /// accidentally-large LLM tool calls reaching the upstream API
    /// unmolested; the desktop tool layer applies the same clamp but the
    /// server re-clamps in depth (defence against a stale desktop
    /// passing an absurd value before that clamp existed).
    /// </summary>
    public const int MaxResultsLimit = 20;
    public const int MinResultsLimit = 1;
    public const int DefaultResultsLimit = 5;

    /// <summary>
    /// Outbound HTTP timeout per upstream call. Matches the desktop tool
    /// layer's value so behaviour is identical regardless of which side
    /// initiated the request.
    /// </summary>
    private static readonly TimeSpan UpstreamTimeout = TimeSpan.FromSeconds(10);

    public WebSearchForwarder(
        IHttpClientFactory httpFactory,
        IServiceScopeFactory scopeFactory,
        ILogger<WebSearchForwarder> logger,
        ISecretProtector secrets)
    {
        _httpFactory = httpFactory;
        _scopeFactory = scopeFactory;
        _logger = logger;
        _secrets = secrets;
    }

    /// <summary>
    /// Wire-side payload returned to the desktop client. Mirrors the
    /// minimal shape the desktop tool needs so it can render the
    /// response identically to a local-Baike call — keeping the JSON
    /// shape stable across providers is the entire reason the desktop
    /// tool's thin `format_*` helpers do not need a new branch.
    /// </summary>
    public record ProviderSearchResult(string ProviderId, string Query, JsonElement Result);

    public record ForwardError(string Code, string Message);

    public record ForwardResult(bool IsSuccess, ProviderSearchResult? Result, ForwardError? Error)
    {
        public static ForwardResult Ok(ProviderSearchResult r) => new(true, r, null);
        public static ForwardResult Err(string code, string msg) => new(false, null, new ForwardError(code, msg));
    }

    public async Task<ForwardResult> ForwardAsync(
        Guid userId,
        string providerId,
        string query,
        int maxResults,
        CancellationToken ct)
    {
        var clampedMaxResults = Math.Clamp(maxResults, MinResultsLimit, MaxResultsLimit);
        if (clampedMaxResults != maxResults)
        {
            _logger.LogDebug(
                "web_search: clamping max_results {Original} -> {Clamped} for user {UserId}",
                maxResults,
                clampedMaxResults,
                userId);
        }

        // Look up the provider from a fresh DI scope so we read the
        // latest operator edits (api-key / base-url / enabled flag) on
        // every call. Caching the entity inside the singleton would mean
        // a key paste in the admin UI takes a process restart to apply.
        using var scope = _scopeFactory.CreateScope();
        var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
        var provider = await db.WebSearchProviders
            .AsNoTracking()
            .FirstOrDefaultAsync(p => p.ProviderId == providerId, ct);

        if (provider is null)
        {
            return ForwardResult.Err(
                "unknown_provider",
                $"web_search provider '{providerId}' is not registered on this cloud server.");
        }

        if (!provider.Enabled)
        {
            return ForwardResult.Err(
                "provider_disabled",
                $"web_search provider '{providerId}' is currently disabled on this cloud server.");
        }

        var apiKey = (_secrets.Unprotect(provider.UpstreamApiKey) ?? string.Empty).Trim();
        if (apiKey.Length == 0)
        {
            return ForwardResult.Err(
                "missing_key",
                $"web_search provider '{providerId}' has no upstream API key configured on this cloud server. "
                + "Ask the operator to add one in the admin panel before retrying.");
        }

        return providerId switch
        {
            "baike" => await DispatchBaikeAsync(provider, query, clampedMaxResults, ct),
            _ => ForwardResult.Err(
                "not_implemented",
                $"web_search provider '{providerId}' is not implemented on this cloud server."),
        };
    }

    /// <summary>
    /// Records a usage row for auditing / abuse-tracing. Wrapped in its
    /// own method so the dispatch helpers can swallow write errors
    /// without failing the user-visible search response.
    /// </summary>
    public async Task RecordUsageAsync(Guid userId, string providerId, string query, CancellationToken ct)
    {
        try
        {
            using var scope = _scopeFactory.CreateScope();
            var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
            db.WebSearchUsageRecords.Add(new WebSearchUsageRecord
            {
                UserId = userId,
                ProviderId = providerId,
                Query = Truncate(query, 512),
            });
            await db.SaveChangesAsync(ct);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to record web_search usage for user {UserId}", userId);
        }
    }

    private static string Truncate(string s, int max) => s.Length <= max ? s : s[..max];

    private async Task<ForwardResult> DispatchBaikeAsync(
        WebSearchProvider provider,
        string query,
        int maxResults,
        CancellationToken ct)
    {
        var baseUrl = string.IsNullOrWhiteSpace(provider.UpstreamBaseUrl)
            ? "https://appbuilder.baidu.com/v2/baike/lemma/get_content"
            : provider.UpstreamBaseUrl!.TrimEnd('/');
        var url = $"{baseUrl}?search_type=lemmaTitle&search_key={Uri.EscapeDataString(query)}";

        var client = _httpFactory.CreateClient("upstream-search");
        // Note: we intentionally do NOT set client.Timeout — that would mutate
        // the pooled instance and affect all concurrent callers. Use a linked CTS
        // instead to apply UpstreamTimeout per-call.
        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeoutCts.CancelAfter(UpstreamTimeout);

        using var req = new HttpRequestMessage(HttpMethod.Get, url);
        req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", _secrets.Unprotect(provider.UpstreamApiKey)?.Trim() ?? string.Empty);
        req.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));
        req.Headers.UserAgent.ParseAdd(
            "inkuo-cloud/1.0 (+https://github.com/inkuo) web_search forwarder");

        HttpResponseMessage response;
        try
        {
            response = await client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, timeoutCts.Token);
        }
        catch (TaskCanceledException) when (!ct.IsCancellationRequested)
        {
            return ForwardResult.Err("upstream_timeout", $"upstream timed out after {UpstreamTimeout.TotalSeconds:F0}s");
        }
        catch (Exception ex)
        {
            return ForwardResult.Err("upstream_network", $"upstream request failed: {ex.Message}");
        }

        var status = (int)response.StatusCode;
        var body = await response.Content.ReadAsStringAsync(ct);

        if (status == 401 || status == 403)
        {
            return ForwardResult.Err(
                "upstream_unauthorized",
                $"upstream rejected the API key (HTTP {status}); verify it in the admin panel.");
        }
        if (status < 200 || status >= 300)
        {
            // Surface a short snippet of the body so the operator sees
            // why the upstream is unhappy in logs / via the desktop
            // error message, but be careful not to leak the entire body
            // (Baidu's CDN occasionally returns HTML error pages with
            // diagnostic info we don't want in front of users).
            var snippet = body.Length > 200 ? body[..200] + "…" : body;
            return ForwardResult.Err("upstream_error", $"upstream returned HTTP {status}: {snippet}");
        }

        JsonElement parsed;
        try
        {
            parsed = JsonDocument.Parse(body).RootElement.Clone();
        }
        catch (Exception ex)
        {
            return ForwardResult.Err("upstream_bad_json", $"upstream response was not valid JSON: {ex.Message}");
        }

        return ForwardResult.Ok(new ProviderSearchResult(provider.ProviderId, query, parsed));
    }
}
