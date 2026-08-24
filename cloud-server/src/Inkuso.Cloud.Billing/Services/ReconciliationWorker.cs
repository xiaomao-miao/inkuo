using System.Security.Claims;
using System.Security.Cryptography;
using System.Text;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Billing.Services;

public class ReconciliationWorker : BackgroundService
{
    private readonly IServiceProvider _sp;
    private readonly ILogger<ReconciliationWorker> _logger;
    private readonly TimeSpan _interval = TimeSpan.FromMinutes(1);
    // The API enforces a five-minute maximum upstream lifetime. Waiting twice
    // that long prevents an active stream from being reclaimed while still
    // bounding holds left by a crashed host.
    private static readonly TimeSpan PendingReservationTtl = TimeSpan.FromMinutes(10);
    private static readonly TimeSpan WebSearchReservationTtl = TimeSpan.FromMinutes(2);

    public ReconciliationWorker(IServiceProvider sp, ILogger<ReconciliationWorker> logger)
    {
        _sp = sp;
        _logger = logger;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                using var scope = _sp.CreateScope();
                var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
                var ledger = scope.ServiceProvider.GetRequiredService<BillingLedger>();
                var searchLedger = scope.ServiceProvider.GetRequiredService<WebSearchLedger>();

                // Mark expired subscriptions
                var expired = await db.Subscriptions
                    .Where(s => s.Status == "active" && s.ExpiresAt <= DateTime.UtcNow)
                    .ToListAsync(stoppingToken);

                foreach (var s in expired)
                {
                    s.Status = "expired";
                    _logger.LogInformation($"Subscription {s.Id} expired for user {s.UserId}");
                }

                if (expired.Count > 0)
                    await db.SaveChangesAsync(stoppingToken);

                var retriedSettlements = 0;
                try
                {
                    retriedSettlements = await ledger.RetryQueuedSettlementsAsync(
                        batchSize: 100,
                        ct: stoppingToken);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to retry one or more queued billing settlements");
                }

                var cutoff = DateTime.UtcNow - PendingReservationTtl;
                var estimatedSettlements = 0;
                try
                {
                    estimatedSettlements = await ledger.SettleStaleStreamsAsync(
                        cutoff,
                        batchSize: 100,
                        ct: stoppingToken);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to settle one or more stale accepted streams");
                }

                // A plain `pending` row never reached a successful upstream
                // response header, so it is safe to release after the same
                // crash-recovery window. Accepted streams use the separate
                // conservative path above and are never silently refunded.
                var releasedReservations = 0;
                try
                {
                    releasedReservations = await ledger.ReleaseStaleAsync(
                        cutoff,
                        batchSize: 100,
                        ct: stoppingToken);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to release one or more stale unstarted reservations");
                }

                var searchCutoff = DateTime.UtcNow - WebSearchReservationTtl;
                var settledSearches = 0;
                var releasedSearches = 0;
                try
                {
                    settledSearches = await searchLedger.SettleStaleStartedAsync(
                        searchCutoff,
                        batchSize: 100,
                        ct: stoppingToken);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to settle one or more stale accepted web searches");
                }
                try
                {
                    releasedSearches = await searchLedger.ReleaseStalePendingAsync(
                        searchCutoff,
                        batchSize: 100,
                        ct: stoppingToken);
                }
                catch (Exception ex)
                {
                    _logger.LogError(ex, "Failed to release one or more stale unstarted web searches");
                }

                _logger.LogInformation(
                    "Reconciliation pass complete: {ExpiredCount} subscriptions expired, {RetriedCount} queued settlements retried, {EstimatedCount} stale streams conservatively settled, {ReleasedCount} unstarted reservations released, {SearchSettledCount} stale searches settled, {SearchReleasedCount} unstarted searches released",
                    expired.Count,
                    retriedSettlements,
                    estimatedSettlements,
                    releasedReservations,
                    settledSearches,
                    releasedSearches);
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "Reconciliation failed");
            }

            try { await Task.Delay(_interval, stoppingToken); }
            catch (TaskCanceledException) { break; }
        }
    }
}

public static class AdminEndpoints
{
    public record CreateRedemptionRequest(string Code, long CreditPoints, Guid? PlanId, int MaxUses);
    public record CreateInviteRequest(string Code, long FreePoints, int MaxUses);

    public static void MapAdminEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/admin").WithTags("admin");

        group.MapPost("/redemption-codes", async (
            [FromHeader(Name = "X-Admin-Token")] string? token,
            CreateRedemptionRequest req,
            AppDbContext db,
            BillingAdminSettings settings) =>
        {
            if (!settings.IsAuthorized(token)) return Results.Unauthorized();

            var code = BillingLimits.NormalizeCode(req.Code);
            var validationError = BillingLimits.ValidateCode(code)
                                  ?? BillingLimits.ValidatePointGrant(req.CreditPoints, allowZero: true)
                                  ?? BillingLimits.ValidateMaxUses(req.MaxUses);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });
            if (req.CreditPoints == 0 && !req.PlanId.HasValue)
                return Results.BadRequest(new { error = "Redemption code must grant points or a plan" });
            if (req.PlanId.HasValue && !await db.Plans.AnyAsync(plan => plan.Id == req.PlanId.Value))
                return Results.BadRequest(new { error = "Invalid PlanId" });
            if (await db.RedemptionCodes.AnyAsync(redemption => redemption.Code == code))
                return Results.Conflict(new { error = "Code already exists" });

            db.RedemptionCodes.Add(new Core.Entities.RedemptionCode
            {
                Code = code,
                CreditPoints = req.CreditPoints,
                PlanId = req.PlanId,
                MaxUses = req.MaxUses,
            });
            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateException)
            {
                return Results.Conflict(new { error = "Code already exists" });
            }
            return Results.Ok(new { code });
        });

        group.MapPost("/invite-codes", async (
            [FromHeader(Name = "X-Admin-Token")] string? token,
            CreateInviteRequest req,
            AppDbContext db,
            BillingAdminSettings settings) =>
        {
            if (!settings.IsAuthorized(token)) return Results.Unauthorized();

            var code = BillingLimits.NormalizeCode(req.Code);
            var validationError = BillingLimits.ValidateCode(code)
                                  ?? BillingLimits.ValidatePointGrant(req.FreePoints, allowZero: true)
                                  ?? BillingLimits.ValidateMaxUses(req.MaxUses);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });
            if (await db.InviteCodes.AnyAsync(invite => invite.Code == code))
                return Results.Conflict(new { error = "Code already exists" });

            db.InviteCodes.Add(new Core.Entities.InviteCode
            {
                Code = code,
                FreePoints = req.FreePoints,
                MaxUses = req.MaxUses,
            });
            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateException)
            {
                return Results.Conflict(new { error = "Code already exists" });
            }
            return Results.Ok(new { code });
        });

        group.MapGet("/stats", async (
            [FromHeader(Name = "X-Admin-Token")] string? token,
            AppDbContext db,
            BillingAdminSettings settings) =>
        {
            if (!settings.IsAuthorized(token)) return Results.Unauthorized();

            var totalUsers = await db.Users.CountAsync();
            var activeSubs = await db.Subscriptions.CountAsync(s => s.Status == "active");
            var monthUsage = await db.UsageRecords
                .Where(u => u.RecordedAt >= new DateTime(DateTime.UtcNow.Year, DateTime.UtcNow.Month, 1, 0, 0, 0, DateTimeKind.Utc)
                            && (u.BillingStatus == "settled"
                                || u.BillingStatus == "estimated"
                                || u.BillingStatus == "debt"))
                .SumAsync(u => (long?)(u.BillingStatus == "debt"
                    ? u.ReservedPoints ?? 0L
                    : u.CostPoints)) ?? 0L;
            var totalRevenue = await db.UsageRecords
                .Where(u => u.BillingStatus == "settled"
                            || u.BillingStatus == "estimated"
                            || u.BillingStatus == "debt")
                .SumAsync(u => (long?)(u.BillingStatus == "debt"
                    ? u.ReservedPoints ?? 0L
                    : u.CostPoints)) ?? 0L;

            return Results.Ok(new
            {
                totalUsers,
                activeSubscriptions = activeSubs,
                monthRevenuePoints = monthUsage,
                totalRevenuePoints = totalRevenue,
            });
        });
    }
}

public sealed class BillingAdminSettings
{
    private readonly byte[] _tokenBytes;

    public BillingAdminSettings(string token)
    {
        _tokenBytes = Encoding.UTF8.GetBytes(token);
    }

    public bool IsAuthorized(string? candidate)
    {
        if (string.IsNullOrEmpty(candidate)) return false;
        var candidateBytes = Encoding.UTF8.GetBytes(candidate);
        return candidateBytes.Length == _tokenBytes.Length
            && CryptographicOperations.FixedTimeEquals(candidateBytes, _tokenBytes);
    }
}
