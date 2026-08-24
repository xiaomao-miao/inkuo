using System.Text.Json;

namespace Inkuso.Cloud.Core.Upstream;

public static class SseUsageParser
{
    public readonly record struct ParsedUsage(
        long PromptTokens,
        long CompletionTokens,
        long CachedPromptTokens);

    public static bool TryParseLine(string line, out ParsedUsage parsed)
    {
        parsed = default;
        var trimmed = line.AsSpan().TrimStart();
        if (!trimmed.StartsWith("data:")) return false;

        var payloadStart = line.IndexOf(':') + 1;
        if (payloadStart <= 0 || payloadStart >= line.Length) return false;
        var payload = line.AsSpan(payloadStart).Trim();
        if (payload.Length == 0 || payload.SequenceEqual("[DONE]")) return false;

        try
        {
            using var document = JsonDocument.Parse(payload.ToString());
            if (!document.RootElement.TryGetProperty("usage", out var usage)
                || usage.ValueKind != JsonValueKind.Object
                || !usage.TryGetProperty("prompt_tokens", out var promptElement)
                || promptElement.ValueKind != JsonValueKind.Number
                || !promptElement.TryGetInt64(out var promptTokens)
                || promptTokens < 0
                || !usage.TryGetProperty("completion_tokens", out var completionElement)
                || completionElement.ValueKind != JsonValueKind.Number
                || !completionElement.TryGetInt64(out var completionTokens)
                || completionTokens < 0)
            {
                return false;
            }

            var cachedTokens = 0L;
            if (usage.TryGetProperty("prompt_tokens_details", out var details)
                && details.ValueKind == JsonValueKind.Object
                && details.TryGetProperty("cached_tokens", out var cachedElement)
                && cachedElement.ValueKind == JsonValueKind.Number
                && cachedElement.TryGetInt64(out var openAiCached)
                && openAiCached >= 0)
            {
                cachedTokens = openAiCached;
            }
            else if (usage.TryGetProperty("cache_read_input_tokens", out var cacheReadElement)
                     && cacheReadElement.ValueKind == JsonValueKind.Number
                     && cacheReadElement.TryGetInt64(out var anthropicCached)
                     && anthropicCached >= 0)
            {
                cachedTokens = anthropicCached;
            }

            parsed = new ParsedUsage(
                promptTokens,
                completionTokens,
                Math.Min(cachedTokens, promptTokens));
            return true;
        }
        catch (JsonException)
        {
            return false;
        }
    }
}
