using System.Security.Claims;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Upstream;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Chat
{
    public static void MapChatEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/v1").WithTags("chat").RequireAuthorization();

        group.MapPost("/chat/completions", async (
            HttpContext ctx,
            AppDbContext db,
            LlmForwarder forwarder,
            BillingLedger ledger,
            ILoggerFactory loggerFactory,
            IHostApplicationLifetime appLifetime,
            CancellationToken ct) =>
        {
            var logger = loggerFactory.CreateLogger("Inkuso.Cloud.Chat");
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);

            ctx.Request.EnableBuffering();
            using var bodyReader = new StreamReader(ctx.Request.Body, Encoding.UTF8, leaveOpen: true);
            var rawBody = await bodyReader.ReadToEndAsync(ct);
            if (string.IsNullOrWhiteSpace(rawBody))
                return Results.BadRequest(new { error = "Empty body" });

            JsonDocument parsedBody;
            try
            {
                parsedBody = JsonDocument.Parse(rawBody);
            }
            catch (JsonException)
            {
                return Results.BadRequest(new { error = "Request body must be valid JSON." });
            }
            using var doc = parsedBody;
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
                return user.AdminSuspended
                    ? Results.Json(
                        new { error = "Account suspended by an administrator. Please contact support." },
                        statusCode: 403)
                    : Results.Json(
                        new { error = "Account suspended due to unpaid usage. Please top up to resume." },
                        statusCode: 402);

            // Estimate the prompt token count from the inbound messages and
            // compute the worst-case cost. We use a generous output cap (the
            // model's MaxOutputTokens field, or 4096 fallback) because the real
            // output size is unknown until the stream completes.
            var approxPromptTokens = EstimatePromptTokens(root);
            var configuredOutputCap = config.MaxOutputTokens > 0 ? config.MaxOutputTokens : 4096;
            var maxOutputCap = ResolveRequestedOutputCap(root, configuredOutputCap);
            var reservePoints = LlmForwarder.EstimateMaxCostPoints(config, approxPromptTokens, maxOutputCap);

            var requestId = ctx.Request.Headers["Idempotency-Key"].FirstOrDefault()?.Trim();
            if (string.IsNullOrWhiteSpace(requestId))
                requestId = Guid.NewGuid().ToString("N");
            if (requestId.Length > 64)
                return Results.BadRequest(new { error = "Idempotency-Key must be at most 64 characters." });

            ctx.Response.Headers["X-Request-Id"] = requestId;
            var reservation = await ledger.TryReserveAsync(
                userId,
                config.Id,
                reservePoints,
                requestId,
                ct,
                new BillingLedger.PricingSnapshot(
                    config.InputPricePerMTokens,
                    config.OutputPricePerMTokens,
                    config.CachedInputPricePerMTokens));
            if (reservation.State == BillingLedger.ReservationState.Rejected)
                return Results.Json(new { error = "Insufficient points balance. Please top up to continue." }, statusCode: 402);
            if (!reservation.CanForward)
                return Results.Conflict(new
                {
                    error = "duplicate_request",
                    request_id = requestId,
                    billing_status = reservation.BillingStatus,
                });

            var releasePendingReservation = true;
            var upstreamAccepted = false;
            var billingAttempted = false;
            var clientConnected = !ctx.RequestAborted.IsCancellationRequested;
            var hasAuthoritativeUsage = false;
            long totalPrompt = 0, totalCompletion = 0, totalCached = 0;
            long streamedUtf8Bytes = 0;

            async Task WriteToClientAsync(string payload)
            {
                if (!clientConnected) return;
                try
                {
                    await ctx.Response.WriteAsync(payload, ctx.RequestAborted);
                    await ctx.Response.Body.FlushAsync(ctx.RequestAborted);
                }
                catch (OperationCanceledException) when (ctx.RequestAborted.IsCancellationRequested)
                {
                    clientConnected = false;
                }
                catch (IOException)
                {
                    clientConnected = false;
                }
                catch (ObjectDisposedException)
                {
                    clientConnected = false;
                }
            }

            async Task FinalizeBillingAsync()
            {
                if (!upstreamAccepted || billingAttempted) return;
                billingAttempted = true;

                // A conforming provider emits an authoritative usage block at
                // the end of the stream. If a timeout/network fault prevents
                // that block, bill a conservative token estimate from the
                // prompt and observed SSE bytes instead of silently making the
                // delivered prefix free. The maximum is still bounded by the
                // request's reserved output cap.
                // Never charge beyond the server-approved hold. A buggy or
                // compromised provider can report token counts larger than the
                // request it accepted; clamping to our conservative input-byte
                // estimate and configured output cap prevents surprise debt.
                if (hasAuthoritativeUsage
                    && (totalPrompt > approxPromptTokens || totalCompletion > maxOutputCap))
                {
                    logger.LogWarning(
                        "Clamped impossible upstream usage: request={RequestId} prompt={Prompt}/{PromptCap} completion={Completion}/{CompletionCap}",
                        requestId,
                        totalPrompt,
                        approxPromptTokens,
                        totalCompletion,
                        maxOutputCap);
                }
                var promptTokens = hasAuthoritativeUsage
                    ? Math.Min(totalPrompt, approxPromptTokens)
                    : approxPromptTokens;
                var completionTokens = hasAuthoritativeUsage
                    ? Math.Min(totalCompletion, (long)maxOutputCap)
                    : Math.Min(
                        (long)maxOutputCap,
                        (streamedUtf8Bytes + 2L) / 3L);
                var cachedTokens = hasAuthoritativeUsage
                    ? Math.Min(totalCached, promptTokens)
                    : 0L;

                try
                {
                    using var queueCts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                    await ledger.QueueSettlementAsync(
                        userId,
                        requestId,
                        promptTokens,
                        completionTokens,
                        cachedTokens,
                        queueCts.Token);
                }
                catch (Exception ex)
                {
                    // The row remains `streaming`; stale reconciliation will
                    // conservatively charge the hold rather than release usage
                    // already accepted by the upstream.
                    logger.LogCritical(ex,
                        "Could not queue delivered usage for settlement: user={UserId} model={ModelId} request={RequestId}",
                        userId, config.Id, requestId);
                    return;
                }

                try
                {
                    using var settleCts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                    var outcome = await ledger.SettleAsync(
                        userId,
                        config.Id,
                        promptTokens,
                        completionTokens,
                        cachedTokens,
                        requestId,
                        settleCts.Token);

                    if (outcome.Status == "debt")
                    {
                        logger.LogWarning(
                            "User suspended for unpaid usage: user={UserId} model={ModelId} cost_points={Cost} missing={Missing}",
                            userId, config.Id, outcome.CostPoints, outcome.DebtPoints);
                    }
                }
                catch (Exception ex)
                {
                    // Usage and token counts are already durable in
                    // `bill_pending`; the billing worker will retry.
                    logger.LogError(ex,
                        "Settlement queued for retry: user={UserId} model={ModelId} request={RequestId}",
                        userId, config.Id, requestId);
                }
            }

            try
            {
                // Rewrite body: pin the upstream model, force streaming, and
                // request a terminal usage block for exact billing.
                var newBody = RewriteRequestBody(root, config.ModelName, maxOutputCap);
                if (ctx.RequestAborted.IsCancellationRequested)
                    return Results.Empty;

                // The complete upstream lifetime is bounded independently of
                // the browser/desktop connection. If the client disconnects we
                // stop writing but keep draining the provider response so the
                // final usage block can still be settled accurately.
                using var upstreamCts = CancellationTokenSource.CreateLinkedTokenSource(
                    appLifetime.ApplicationStopping);
                upstreamCts.CancelAfter(LlmForwarder.MaxStreamDuration);
                using var forwardResult = await forwarder.ForwardStreamAsync(
                    userId, config.Id, newBody, upstreamCts.Token);

                using (var markCts = new CancellationTokenSource(TimeSpan.FromSeconds(15)))
                {
                    var marked = await ledger.MarkStreamingAsync(userId, requestId, markCts.Token);
                    if (!marked)
                        throw new BillingInvariantException(
                            $"Reservation {requestId} was not pending when the upstream accepted it.");
                }
                upstreamAccepted = true;
                releasePendingReservation = false;

                ctx.Response.ContentType = "text/event-stream";
                ctx.Response.Headers.CacheControl = "no-cache";
                ctx.Response.Headers.Connection = "keep-alive";

                using var reader = new StreamReader(
                    forwardResult.UpstreamStream,
                    Encoding.UTF8,
                    detectEncodingFromByteOrderMarks: false);
                var carry = new StringBuilder();
                var buffer = new char[8192];

                try
                {
                    int read;
                    while ((read = await reader.ReadAsync(
                               buffer.AsMemory(),
                               upstreamCts.Token)) > 0)
                    {
                        streamedUtf8Bytes += Encoding.UTF8.GetByteCount(buffer.AsSpan(0, read));
                        var chunk = carry.ToString() + new string(buffer, 0, read);
                        int newlineIdx;
                        var startIdx = 0;
                        while ((newlineIdx = chunk.IndexOf('\n', startIdx)) >= 0)
                        {
                            var line = chunk.Substring(startIdx, newlineIdx - startIdx);
                            startIdx = newlineIdx + 1;
                            hasAuthoritativeUsage |= AccumulateUsage(
                                line,
                                ref totalPrompt,
                                ref totalCompletion,
                                ref totalCached);
                            await WriteToClientAsync(line + "\n");
                        }
                        carry.Clear();
                        if (startIdx < chunk.Length)
                            carry.Append(chunk, startIdx, chunk.Length - startIdx);
                    }
                    if (carry.Length > 0)
                    {
                        var finalLine = carry.ToString();
                        hasAuthoritativeUsage |= AccumulateUsage(
                            finalLine,
                            ref totalPrompt,
                            ref totalCompletion,
                            ref totalCached);
                        await WriteToClientAsync(finalLine);
                    }
                }
                catch (OperationCanceledException) when (upstreamCts.IsCancellationRequested)
                {
                    logger.LogWarning(
                        "Upstream stream reached its server-controlled limit: request={RequestId} limit_seconds={Limit}",
                        requestId,
                        LlmForwarder.MaxStreamDuration.TotalSeconds);
                }
                catch (Exception ex)
                {
                    logger.LogWarning(ex,
                        "Upstream stream ended before a complete usage block: request={RequestId}",
                        requestId);
                }

                await FinalizeBillingAsync();
                return Results.Empty;
            }
            catch (Exception ex)
            {
                await FinalizeBillingAsync();
                logger.LogError(ex,
                    "Chat request failed after reservation: user={UserId} model={ModelId} request={RequestId}",
                    userId, config.Id, requestId);
                if (!ctx.Response.HasStarted)
                    return Results.Json(
                        new { error = "upstream_unavailable", request_id = requestId },
                        statusCode: 502);
                return Results.Empty;
            }
            finally
            {
                if (releasePendingReservation)
                {
                    try
                    {
                        using var releaseCts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                        await ledger.ReleaseAsync(userId, requestId, releaseCts.Token);
                    }
                    catch (Exception ex)
                    {
                        logger.LogCritical(ex,
                            "Reservation cleanup failed: user={UserId} request={RequestId}",
                            userId, requestId);
                    }
                }
            }
        });
    }

    /// <summary>
    /// Rough token estimate for the inbound messages list. We don't have a real
    /// tokenizer here, so we reserve one token per UTF-8 byte of the complete
    /// request JSON. This deliberately overestimates ASCII and CJK prompts, but
    /// prevents under-reserving tool definitions, multimodal metadata or Chinese.
    /// </summary>
    private static int EstimatePromptTokens(JsonElement root)
    {
        return Encoding.UTF8.GetByteCount(root.GetRawText());
    }

    private static int ResolveRequestedOutputCap(JsonElement root, int configuredCap)
    {
        var requested = 0;
        foreach (var propertyName in new[] { "max_tokens", "max_completion_tokens" })
        {
            if (!root.TryGetProperty(propertyName, out var value)
                || value.ValueKind != JsonValueKind.Number
                || !value.TryGetInt32(out var parsed))
                continue;
            requested = Math.Max(requested, Math.Clamp(parsed, 1, configuredCap));
        }
        return requested > 0 ? requested : configuredCap;
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
    private static string RewriteRequestBody(
        JsonElement root,
        string upstreamModelName,
        int maxOutputTokens)
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
                    foreach (var sub in prop.Value.EnumerateObject())
                    {
                        if (!sub.NameEquals("include_usage")) sub.WriteTo(writer);
                    }
                    writer.WriteBoolean("include_usage", true);
                    writer.WriteEndObject();
                }
                else if (prop.NameEquals("max_tokens") || prop.NameEquals("max_completion_tokens"))
                {
                    var requested = prop.Value.ValueKind == JsonValueKind.Number
                        && prop.Value.TryGetInt32(out var parsed)
                        ? parsed
                        : maxOutputTokens;
                    writer.WriteNumber(prop.Name, Math.Clamp(requested, 1, maxOutputTokens));
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
            if (!root.TryGetProperty("max_tokens", out _)
                && !root.TryGetProperty("max_completion_tokens", out _))
                writer.WriteNumber("max_tokens", maxOutputTokens);
            writer.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    /// <summary>
    /// Parse a single SSE line and, if it is a <c>data:</c> payload (other
    /// than <c>[DONE]</c>), extract any usage block and aggregate it into the
    /// caller's running totals.
    /// </summary>
    private static bool AccumulateUsage(
        string line,
        ref long totalPrompt,
        ref long totalCompletion,
        ref long totalCached)
    {
        if (!SseUsageParser.TryParseLine(line, out var usage)) return false;
        totalPrompt = usage.PromptTokens;
        totalCompletion = usage.CompletionTokens;
        totalCached = usage.CachedPromptTokens;
        return true;
    }
}
