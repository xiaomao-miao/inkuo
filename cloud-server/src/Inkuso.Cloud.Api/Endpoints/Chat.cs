using System.Security.Claims;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Upstream;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Chat
{
    /// <summary>
    /// SSE line pattern: a complete `data: ...` block sitting on its own
    /// line. We use this to slice out the JSON payload even when a chunk
    /// straddles the 8 KiB read buffer.
    /// </summary>
    private static readonly Regex DataLineRegex = new(
        @"^data:\s*(?<payload>.*?)\s*$",
        RegexOptions.Compiled | RegexOptions.Multiline);

    public static void MapChatEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/v1").WithTags("chat").RequireAuthorization();

        group.MapPost("/chat/completions", async (
            HttpContext ctx,
            AppDbContext db,
            LlmForwarder forwarder,
            ILoggerFactory loggerFactory,
            CancellationToken ct) =>
        {
            var logger = loggerFactory.CreateLogger("Inkuso.Cloud.Chat");
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);

            ctx.Request.EnableBuffering();
            using var bodyReader = new StreamReader(ctx.Request.Body, Encoding.UTF8, leaveOpen: true);
            var rawBody = await bodyReader.ReadToEndAsync(ct);
            if (string.IsNullOrWhiteSpace(rawBody))
                return Results.BadRequest(new { error = "Empty body" });

            using var doc = JsonDocument.Parse(rawBody);
            var root = doc.RootElement;

            var requestedModel = root.TryGetProperty("model", out var m) ? m.GetString() : null;
            if (string.IsNullOrWhiteSpace(requestedModel))
                return Results.BadRequest(new { error = "model is required" });

            // Resolve which model_config the user wants:
            // - prefer matching model_config.Id (guid)
            // - fallback to upstream ModelName
            ModelConfig? config = null;
            if (Guid.TryParse(requestedModel, out var configId))
            {
                config = await db.ModelConfigs.FirstOrDefaultAsync(c => c.Id == configId && c.Enabled, ct);
            }
            if (config is null)
            {
                config = await db.ModelConfigs.FirstOrDefaultAsync(c => c.ModelName == requestedModel && c.Enabled, ct);
            }
            if (config is null)
                return Results.BadRequest(new { error = $"Model '{requestedModel}' not found or disabled" });

            // --- Quota gate ---
            // The account uses points (1 元 = 1000 点). The user must have enough
            // unreserved points to cover the worst-case charge for this request.
            // We check BalancePoints - ReservedPoints rather than BalancePoints
            // alone so an in-flight request that's already reserved a chunk
            // doesn't get a second, overlapping reservation.
            var user = await db.Users.FindAsync(userId);
            if (user is null) return Results.Unauthorized();
            if (user.IsSuspended)
                return Results.Json(new { error = "Account suspended due to unpaid usage. Please top up to resume." }, statusCode: 402);

            // Estimate the prompt token count from the inbound messages and
            // compute the worst-case cost. We use a generous output cap (the
            // model's MaxOutputTokens field, or 4096 fallback) because the real
            // output size is unknown until the stream completes.
            var approxPromptTokens = EstimatePromptTokens(root);
            var maxOutputCap = config.MaxOutputTokens > 0 ? config.MaxOutputTokens : 4096;
            var reservePoints = LlmForwarder.EstimateMaxCostPoints(config, approxPromptTokens, maxOutputCap);

            var requestId = Guid.NewGuid().ToString("N");
            var reserved = await forwarder.TryReservePointsAsync(userId, config.Id, reservePoints, requestId, ct);
            if (reserved == 0)
                return Results.Json(new { error = "Insufficient points balance. Please top up to continue." }, statusCode: 402);

            // Rewrite body: force upstream model name + force stream=true +
            // request usage blocks so we can bill. We mutate the original
            // JsonElement tree (instead of round-tripping through object→string
            // →object which would change number precision and string escaping),
            // then serialize once.
            var newBody = RewriteRequestBody(root, config.ModelName);

            // Call upstream
            var forwardResult = await forwarder.ForwardStreamAsync(userId, config.Id, newBody, ct);
            var upstreamStream = forwardResult.UpstreamStream;

            ctx.Response.ContentType = "text/event-stream";
            ctx.Response.Headers.CacheControl = "no-cache";
            ctx.Response.Headers.Connection = "keep-alive";

            using var reader = new StreamReader(upstreamStream, Encoding.UTF8, detectEncodingFromByteOrderMarks: false);
            // Reassemble SSE lines across chunk boundaries. The previous
            // implementation called `chunk.Split('\n')` directly which dropped
            // every `data:` line that straddled the 8 KiB network read buffer;
            // that silently lost the usage block and meant real customer usage
            // was never billed. The accumulator holds the partial line that
            // didn't end in '\n' yet.
            var carry = new StringBuilder();
            long totalPrompt = 0, totalCompletion = 0, totalCached = 0;
            var buffer = new char[8192];
            int read;
            int linesProcessed = 0;

            try
            {
                while ((read = await reader.ReadAsync(buffer.AsMemory(), ct)) > 0)
                {
                    var chunk = carry + new string(buffer, 0, read);
                    int newlineIdx;
                    int startIdx = 0;
                    while ((newlineIdx = chunk.IndexOf('\n', startIdx)) >= 0)
                    {
                        var line = chunk.Substring(startIdx, newlineIdx - startIdx);
                        startIdx = newlineIdx + 1;
                        linesProcessed++;
                        await ctx.Response.WriteAsync(line + "\n", ct);
                        await ctx.Response.Body.FlushAsync(ct);

                        AccumulateUsage(line, ref totalPrompt, ref totalCompletion, ref totalCached);
                    }
                    // Anything past the last newline is a partial line; stash it
                    // for the next iteration's prefix.
                    carry.Clear();
                    if (startIdx < chunk.Length) carry.Append(chunk, startIdx, chunk.Length - startIdx);
                }
                // Flush any final carry (a stream that didn't end with a newline).
                if (carry.Length > 0)
                {
                    await ctx.Response.WriteAsync(carry.ToString(), ct);
                    AccumulateUsage(carry.ToString(), ref totalPrompt, ref totalCompletion, ref totalCached);
                    await ctx.Response.Body.FlushAsync(ct);
                }
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                // Client disconnected: refund the reservation rather than leaving
                // it held. We don't bill partially-received usage because the
                // account is in an unsettled state and would otherwise lock out
                // unrelated subsequent requests.
                try
                {
                    await forwarder.ReleaseReservationAsync(userId, reservePoints, requestId, CancellationToken.None);
                }
                catch (Exception ex)
                {
                    logger.LogError(ex, "Failed to release reservation after client disconnect: user={UserId}", userId);
                }
                return Results.Empty;
            }

            // --- Settle billing ---
            // Even if the upstream returned zero tokens we still write a settled
            // row so the audit trail reflects the full lifecycle. The
            // Reservation is released (refunded) and the actual cost is debited.
            try
            {
                var outcome = await forwarder.SettleUsageAsync(
                    userId,
                    config.Id,
                    totalPrompt,
                    totalCompletion,
                    totalCached,
                    reservePoints,
                    requestId,
                    ct);

                if (outcome.Status == "debt")
                {
                    // Surface the suspension to the desktop client so the next
                    // /account/me refetch shows the suspended state. The body
                    // may already have been fully streamed at this point, so we
                    // log it instead of trying to rewrite the HTTP status.
                    logger.LogWarning(
                        "User suspended for unpaid usage: user={UserId} model={ModelId} cost_points={Cost} missing={Missing}",
                        userId, config.Id, outcome.CostPoints, outcome.MissingPoints);
                    await ctx.Response.WriteAsync(
                        $"data: {{\"error\":\"insufficient_points\",\"message\":\"usage_billed_but_balance_short\",\"suspended\":true}}\n\n",
                        ct);
                    await ctx.Response.Body.FlushAsync(ct);
                }
            }
            catch (Exception ex)
            {
                logger.LogError(ex,
                    "Failed to settle usage: user={UserId} model={ModelId} reserved={Reserved}",
                    userId, config.Id, reservePoints);
            }

            return Results.Empty;
        });
    }

    /// <summary>
    /// Rough token estimate for the inbound messages list. We don't have a real
    /// tokenizer here, so we use a conservative 4 chars/token heuristic
    /// (CJK-heavy 中文 typically estimates closer to 1.5 chars/token, but 4 is
    /// safe for "this won't bill less than actual"). Returning a low estimate
    /// would short-change the reservation and let the user bypass the gate.
    /// </summary>
    private static int EstimatePromptTokens(JsonElement root)
    {
        if (!root.TryGetProperty("messages", out var messages) || messages.ValueKind != JsonValueKind.Array)
            return 0;
        int totalChars = 0;
        foreach (var message in messages.EnumerateArray())
        {
            if (message.TryGetProperty("content", out var content))
            {
                if (content.ValueKind == JsonValueKind.String)
                {
                    totalChars += content.GetString()?.Length ?? 0;
                }
                else if (content.ValueKind == JsonValueKind.Array)
                {
                    // OpenAI multimodal: array of {type, text} parts.
                    foreach (var part in content.EnumerateArray())
                    {
                        if (part.TryGetProperty("text", out var text))
                            totalChars += text.GetString()?.Length ?? 0;
                    }
                }
            }
        }
        return totalChars / 4;
    }

    /// <summary>
    /// Rewrite the inbound chat-completions body: pin <c>model</c> to the
    /// upstream name resolved from the matching ModelConfig and force
    /// <c>stream=true</c>. We also inject <c>stream_options.include_usage</c>
    /// so the upstream emits a usage block that we can bill — without that
    /// flag a provider that follows the OpenAI spec strictly returns no
    /// usage data and the actual token cost would never reach the
    /// settlement code.
    /// </summary>
    private static string RewriteRequestBody(JsonElement root, string upstreamModelName)
    {
        using var ms = new MemoryStream();
        using (var writer = new Utf8JsonWriter(ms))
        {
            writer.WriteStartObject();
            foreach (var prop in root.EnumerateObject())
            {
                if (prop.NameEquals("model"))
                {
                    writer.WriteString("model", upstreamModelName);
                }
                else if (prop.NameEquals("stream"))
                {
                    writer.WriteBoolean("stream", true);
                }
                else if (prop.NameEquals("stream_options"))
                {
                    // Merge the client's stream_options with the required include_usage flag.
                    writer.WriteStartObject("stream_options");
                    foreach (var sub in prop.Value.EnumerateObject()) sub.WriteTo(writer);
                    writer.WriteBoolean("include_usage", true);
                    writer.WriteEndObject();
                }
                else
                {
                    prop.WriteTo(writer);
                }
            }
            // Defensive defaults — if the client omitted these we still want
            // a sane request shape, but never overwrite an explicit client value.
            if (!root.TryGetProperty("stream", out _))
                writer.WriteBoolean("stream", true);
            if (!root.TryGetProperty("stream_options", out _))
            {
                writer.WriteStartObject("stream_options");
                writer.WriteBoolean("include_usage", true);
                writer.WriteEndObject();
            }
            writer.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    /// <summary>
    /// Parse a single SSE line and, if it is a <c>data:</c> payload (other
    /// than <c>[DONE]</c>), extract any usage block and aggregate it into the
    /// caller's running totals.
    /// </summary>
    private static void AccumulateUsage(
        string line,
        ref long totalPrompt,
        ref long totalCompletion,
        ref long totalCached)
    {
        var trimmed = line.AsSpan().TrimStart();
        if (!trimmed.StartsWith("data:"))
            return;

        // Skip everything after the first colon, then strip optional leading
        // whitespace — SSE spec leaves a single space after the colon for
        // compatibility, but clients should also accept none.
        var payloadStart = line.IndexOf(':') + 1;
        if (payloadStart <= 0 || payloadStart >= line.Length) return;
        var payload = line.AsSpan(payloadStart).Trim();
        if (payload.Length == 0) return;
        if (payload.SequenceEqual("[DONE]")) return;

        try
        {
            using var chunkDoc = JsonDocument.Parse(payload.ToString());
            if (!chunkDoc.RootElement.TryGetProperty("usage", out var usage))
                return;
            if (usage.TryGetProperty("prompt_tokens", out var pt))
                totalPrompt = pt.GetInt64();
            if (usage.TryGetProperty("completion_tokens", out var ct2))
                totalCompletion = ct2.GetInt64();
            // OpenAI-style: usage.prompt_tokens_details.cached_tokens
            // Anthropic-style: usage.cache_read_input_tokens (counts toward prompt_tokens)
            if (usage.TryGetProperty("prompt_tokens_details", out var ptd) &&
                ptd.ValueKind == JsonValueKind.Object &&
                ptd.TryGetProperty("cached_tokens", out var cached))
            {
                totalCached = cached.GetInt64();
            }
            else if (usage.TryGetProperty("cache_read_input_tokens", out var cr))
            {
                totalCached = cr.GetInt64();
            }
        }
        catch
        {
            // Malformed SSE line — ignore; upstream might emit heartbeats or
            // mid-stream annotations that we don't care about.
        }
    }
}
