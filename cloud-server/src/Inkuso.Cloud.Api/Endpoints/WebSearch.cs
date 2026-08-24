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
using Inkuso.Cloud.Core.Billing;
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
            WebSearchForwarder forwarder,
            WebSearchLedger ledger,
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
            var providerId = req.Provider.Trim().ToLowerInvariant();
            var query = req.Query.Trim();
            if (providerId.Length > 64)
                return Results.BadRequest(new { error = "provider must be at most 64 characters" });
            if (query.Length > 512)
                return Results.BadRequest(new { error = "query must be at most 512 characters" });

            // A search call costs a flat 50 points (¥0.05). Reserve it in a
            // durable, idempotent audit row before contacting the provider.
            const long WebSearchCostPoints = 50;
            var requestId = ctx.Request.Headers["Idempotency-Key"].FirstOrDefault()?.Trim();
            if (string.IsNullOrWhiteSpace(requestId))
                requestId = Guid.NewGuid().ToString("N");
            if (requestId.Length > 64)
                return Results.BadRequest(new { error = "Idempotency-Key must be at most 64 characters" });
            ctx.Response.Headers["X-Request-Id"] = requestId;

            var reservation = await ledger.TryReserveAsync(
                userId,
                providerId,
                query,
                WebSearchCostPoints,
                requestId,
                ct);
            if (reservation.State == WebSearchLedger.ReservationState.Rejected)
            {
                return reservation.RejectionReason switch
                {
                    "admin_suspended" => Results.Json(
                        new { error = "Account suspended by an administrator. Please contact support." },
                        statusCode: 403),
                    "billing_suspended" => Results.Json(
                        new { error = "Account suspended due to unpaid usage. Please top up to resume." },
                        statusCode: 402),
                    "account_unavailable" => Results.Unauthorized(),
                    _ => Results.Json(
                        new { error = "Insufficient points balance. Please top up to continue." },
                        statusCode: 402),
                };
            }
            if (!reservation.CanForward)
                return Results.Conflict(new
                {
                    error = "duplicate_request",
                    request_id = requestId,
                    billing_status = reservation.BillingStatus,
                });

            var releaseReservation = true;
            var upstreamAccepted = false;
            var logger = loggerFactory.CreateLogger("Inkuso.Cloud.WebSearch");
            try
            {
                var forward = await forwarder.ForwardAsync(
                    userId,
                    providerId,
                    query,
                    req.MaxResults ?? 5,
                    async () =>
                    {
                        using var markCts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                        if (!await ledger.MarkStartedAsync(userId, requestId, markCts.Token))
                            throw new BillingInvariantException(
                                $"Web search reservation {requestId} could not enter started state.");
                        upstreamAccepted = true;
                    },
                    ct);
                if (!forward.IsSuccess || forward.Result is null)
                {
                    var (status, payload) = MapErrorToHttp(forward.Error!);
                    return Results.Json(payload, statusCode: status);
                }

                if (!upstreamAccepted)
                    throw new BillingInvariantException(
                        $"Web search {requestId} returned data without an accepted billing transition.");

                // From this point onward a result has been produced. If the
                // immediate settlement fails, leave `started` durable so the
                // background worker charges it; never refund delivered data.
                releaseReservation = false;
                try
                {
                    using var settleCts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                    await ledger.SettleAsync(userId, requestId, settleCts.Token);
                }
                catch (Exception ex)
                {
                    logger.LogCritical(ex,
                        "Web search settlement deferred to reconciliation: user={UserId} request={RequestId}",
                        userId,
                        requestId);
                }

                return Results.Ok(new
                {
                    provider = forward.Result.ProviderId,
                    query = forward.Result.Query,
                    result = forward.Result.Result,
                });
            }
            finally
            {
                if (releaseReservation)
                {
                    try
                    {
                        using var releaseCts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                        await ledger.ReleaseAsync(userId, requestId, releaseCts.Token);
                    }
                    catch (Exception ex)
                    {
                        logger.LogCritical(ex,
                            "Failed to release unsuccessful web_search reservation: user={UserId} request={RequestId}",
                            userId,
                            requestId);
                    }
                }
            }
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
