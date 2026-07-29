// <copyright file="WebSearch.cs" company="inkuo">
// Customer-facing web_search endpoint. Mirrors `Chat.cs`:
//   1. Validates the user's JWT (via `RequireAuthorization()` on the group).
//   2. Enforces subscription-or-balance quota (one count per call).
//   3. Delegates the actual upstream call to `WebSearchForwarder`.
//   4. Persists a usage row for auditing / abuse-tracing.
//
// The desktop client treats this endpoint as a drop-in for the local
// Baike provider: the response shape is the same `result`/`error`
// envelope, so the user-facing renderer does not need a new branch.
// </copyright>

using System.Security.Claims;
using System.Text.Json;
using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Upstream;

namespace Inkuso.Cloud.Api.Endpoints;

public static class WebSearch
{
    public record WebSearchRequest(
        string Provider,
        string Query,
        int? MaxResults);

    public static void MapWebSearchEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/v1").WithTags("web_search").RequireAuthorization();

        group.MapPost("/web_search", async (
            HttpContext ctx,
            WebSearchRequest req,
            AppDbContext db,
            WebSearchForwarder forwarder,
            ILoggerFactory loggerFactory,
            CancellationToken ct) =>
        {
            var userIdRaw = ctx.User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                            ?? ctx.User.FindFirst("sub")?.Value;
            if (!Guid.TryParse(userIdRaw, out var userId))
                return Results.Unauthorized();

            if (req is null)
                return Results.BadRequest(new { error = "Empty body" });
            if (string.IsNullOrWhiteSpace(req.Provider))
                return Results.BadRequest(new { error = "provider is required" });
            if (string.IsNullOrWhiteSpace(req.Query))
                return Results.BadRequest(new { error = "query is required" });

            // Quota gate: a search call reserves a flat rate of web-search points
            // (50 points = ¥0.05 / call) regardless of how many results the
            // upstream returns. The gate mirrors `Chat`'s reservation pattern so
            // the user's quotas can be reasoned about in one place. Without
            // the gate a desynced client could keep poking web_search until
            // their balance ran out without anyone noticing.
            const long WebSearchCostPoints = 50;
            var user = await db.Users.FindAsync(userId);
            if (user is null) return Results.Unauthorized();
            if (user.IsSuspended || user.BalancePoints - user.ReservedPoints < WebSearchCostPoints)
                return Results.Json(new { error = "Insufficient points balance. Please top up to continue." }, statusCode: 402);
            await db.Users
                .Where(u => u.Id == userId)
                .ExecuteUpdateAsync(s => s.SetProperty(u => u.ReservedPoints, u => u.ReservedPoints + WebSearchCostPoints), ct);

            var forward = await forwarder.ForwardAsync(userId, req.Provider, req.Query, req.MaxResults ?? 5, ct);
            if (!forward.IsSuccess || forward.Result is null)
            {
                var (status, payload) = MapErrorToHttp(forward.Error!);
                return Results.Json(payload, statusCode: status);
            }

            // Fire-and-forget audit log so a search that takes 200ms
            // upstream doesn't pay the write penalty. RecordUsageAsync
            // already catches its own exceptions, but we attach a
            // continuation that surfaces unobserved exceptions to the
            // ASP.NET unhandled-exception logger (the previous code
            // silently dropped them on shutdown).
            var auditTask = forwarder.RecordUsageAsync(userId, req.Provider, req.Query, CancellationToken.None);
            _ = auditTask.ContinueWith(
                t => loggerFactory.CreateLogger("Inkuso.Cloud.WebSearch").LogWarning(
                    t.Exception, "Unobserved exception in web_search audit log"),
                TaskContinuationOptions.OnlyOnFaulted);

            // Release the reservation and apply the actual flat-rate cost
            // (50 points per call). We can't go through LlmForwarder here
            // because web_search has no per-token dimension.
            try
            {
                await db.Users
                    .Where(u => u.Id == userId)
                    .ExecuteUpdateAsync(s => s
                        .SetProperty(u => u.ReservedPoints, u => u.ReservedPoints - WebSearchCostPoints)
                        .SetProperty(u => u.BalancePoints, u => u.BalancePoints - WebSearchCostPoints));
            }
            catch (Exception ex)
            {
                loggerFactory.CreateLogger("Inkuso.Cloud.WebSearch").LogError(ex,
                    "Failed to settle web_search usage: user={UserId}", userId);
            }

            return Results.Ok(new
            {
                provider = forward.Result.ProviderId,
                query = forward.Result.Query,
                result = forward.Result.Result,
            });
        });
    }

    /// <summary>
    /// Map an upstream-forwarder error code to a sensible (HTTP status,
    /// payload) pair. We translate the most common cases to specific
    /// HTTP codes (404 / 503 / 502) so the desktop client can render a
    /// tighter message; everything else collapses to 502 ("bad
    /// gateway") so the client can branch on `error` field text instead
    /// of chasing yet another status enum.
    /// </summary>
    private static (int StatusCode, object Payload) MapErrorToHttp(WebSearchForwarder.ForwardError err)
    {
        return err.Code switch
        {
            "unknown_provider" => (404, new { error = err.Message }),
            "provider_disabled" => (503, new { error = err.Message }),
            "missing_key" => (503, new { error = err.Message }),
            "upstream_unauthorized" => (502, new { error = err.Message, code = err.Code }),
            "upstream_timeout" => (504, new { error = err.Message, code = err.Code }),
            "upstream_network" => (502, new { error = err.Message, code = err.Code }),
            "upstream_error" => (502, new { error = err.Message, code = err.Code }),
            "upstream_bad_json" => (502, new { error = err.Message, code = err.Code }),
            _ => (502, new { error = err.Message, code = err.Code }),
        };
    }
}
