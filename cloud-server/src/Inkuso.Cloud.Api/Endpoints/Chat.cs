using System.Security.Claims;
using System.Text.Json;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
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
            CancellationToken ct) =>
        {
            var userId = Guid.Parse(ctx.User.FindFirst(ClaimTypes.NameIdentifier)!.Value);

            ctx.Request.EnableBuffering();
            var rawBody = await new StreamReader(ctx.Request.Body).ReadToEndAsync(ct);
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

            // Quota check
            var hasSub = await db.Subscriptions.AnyAsync(s =>
                s.UserId == userId && s.Status == "active" && s.ExpiresAt > DateTime.UtcNow, ct);
            var user = await db.Users.FindAsync(userId);
            if (user is null) return Results.Unauthorized();
            if (!hasSub && user.BalanceCents <= 0)
                return Results.Json(new { error = "No active subscription or balance" }, statusCode: 402);

            // Rewrite body: force upstream model name + force stream=true
            using var outDoc = JsonDocument.Parse(rawBody);
            var outDict = new Dictionary<string, object?>();
            foreach (var prop in outDoc.RootElement.EnumerateObject())
            {
                if (prop.Name == "model")
                    outDict["model"] = config.ModelName;
                else
                    outDict[prop.Name] = JsonSerializer.Deserialize<object>(prop.Value.GetRawText());
            }
            outDict["stream"] = true;
            var newBody = JsonSerializer.Serialize(outDict);

            // Call upstream
            var forwardResult = await forwarder.ForwardStreamAsync(userId, config.Id, newBody, ct);
            var upstreamStream = forwardResult.UpstreamStream;

            // Stream upstream SSE → client, and parse usage block
            ctx.Response.ContentType = "text/event-stream";
            ctx.Response.Headers.CacheControl = "no-cache";
            ctx.Response.Headers.Connection = "keep-alive";

            using var reader = new StreamReader(upstreamStream);
            long totalPrompt = 0, totalCompletion = 0, totalCached = 0;
            var buffer = new char[8192];
            int read;

            while ((read = await reader.ReadAsync(buffer.AsMemory(), ct)) > 0)
            {
                var chunk = new string(buffer, 0, read);
                await ctx.Response.WriteAsync(chunk, ct);
                await ctx.Response.Body.FlushAsync(ct);

                foreach (var line in chunk.Split('\n'))
                {
                    if (line.StartsWith("data:") && !line.Contains("[DONE]"))
                    {
                        var jsonPart = line.Substring(5).Trim();
                        try
                        {
                            using var chunkDoc = JsonDocument.Parse(jsonPart);
                            if (chunkDoc.RootElement.TryGetProperty("usage", out var usage))
                            {
                                if (usage.TryGetProperty("prompt_tokens", out var pt))
                                    totalPrompt = pt.GetInt64();
                                if (usage.TryGetProperty("completion_tokens", out var ct2))
                                    totalCompletion = ct2.GetInt64();
                                // OpenAI-style: usage.prompt_tokens_details.cached_tokens
                                // Anthropic-style: usage.cache_read_input_tokens (counts toward prompt_tokens)
                                if (usage.TryGetProperty("prompt_tokens_details", out var ptd) &&
                                    ptd.ValueKind == JsonValueKind.Object &&
                                    ptd.TryGetProperty("cached_tokens", out var cached))
                                    totalCached = cached.GetInt64();
                                else if (usage.TryGetProperty("cache_read_input_tokens", out var cr))
                                    totalCached = cr.GetInt64();
                            }
                        }
                        catch { /* ignore malformed SSE line */ }
                    }
                }
            }

            if (totalPrompt > 0 || totalCompletion > 0)
            {
                try
                {
                    await forwarder.RecordUsageAsync(userId, config.Id, totalPrompt, totalCompletion, totalCached, ct);
                }
                catch (Exception ex)
                {
                    Console.WriteLine($"[Billing] Failed to record usage: {ex.Message}");
                }
            }

            return Results.Empty;
        });
    }
}