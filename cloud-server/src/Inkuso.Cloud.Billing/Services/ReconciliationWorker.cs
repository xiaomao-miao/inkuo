using System.Security.Claims;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Billing.Services;

public class ReconciliationWorker : BackgroundService
{
    private readonly IServiceProvider _sp;
    private readonly ILogger<ReconciliationWorker> _logger;
    private readonly TimeSpan _interval = TimeSpan.FromMinutes(15);

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

                _logger.LogInformation($"Reconciliation pass complete: {expired.Count} subscriptions expired");
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
    public record CreateRedemptionRequest(string Code, decimal CreditCents, Guid? PlanId, int MaxUses);
    public record CreateInviteRequest(string Code, decimal FreeQuotaCents, int MaxUses);

    public static void MapAdminEndpoints(this WebApplication app)
    {
        // Internal admin endpoints - in production these need extra protection
        // (separate admin token, IP whitelist, etc.). For V1 we use a simple admin token from env.
        var adminToken = app.Configuration["Admin:Token"] ?? "dev-admin-token-change-me";

        var group = app.MapGroup("/admin").WithTags("admin");

        group.MapPost("/redemption-codes", async (
            [FromHeader(Name = "X-Admin-Token")] string? token,
            CreateRedemptionRequest req,
            AppDbContext db) =>
        {
            if (token != adminToken) return Results.Unauthorized();

            db.RedemptionCodes.Add(new Core.Entities.RedemptionCode
            {
                Code = req.Code,
                CreditCents = req.CreditCents,
                PlanId = req.PlanId,
                MaxUses = req.MaxUses,
            });
            await db.SaveChangesAsync();
            return Results.Ok(new { code = req.Code });
        });

        group.MapPost("/invite-codes", async (
            [FromHeader(Name = "X-Admin-Token")] string? token,
            CreateInviteRequest req,
            AppDbContext db) =>
        {
            if (token != adminToken) return Results.Unauthorized();

            db.InviteCodes.Add(new Core.Entities.InviteCode
            {
                Code = req.Code,
                FreeQuotaCents = req.FreeQuotaCents,
                MaxUses = req.MaxUses,
            });
            await db.SaveChangesAsync();
            return Results.Ok(new { code = req.Code });
        });

        group.MapGet("/stats", async (
            [FromHeader(Name = "X-Admin-Token")] string? token,
            AppDbContext db) =>
        {
            if (token != adminToken) return Results.Unauthorized();

            var totalUsers = await db.Users.CountAsync();
            var activeSubs = await db.Subscriptions.CountAsync(s => s.Status == "active");
            var monthUsage = await db.UsageRecords
                .Where(u => u.RecordedAt >= new DateTime(DateTime.UtcNow.Year, DateTime.UtcNow.Month, 1, 0, 0, 0, DateTimeKind.Utc))
                .SumAsync(u => (decimal?)u.CostCents) ?? 0;
            var totalRevenue = await db.UsageRecords.SumAsync(u => (decimal?)u.CostCents) ?? 0;

            return Results.Ok(new
            {
                totalUsers,
                activeSubscriptions = activeSubs,
                monthRevenueCents = monthUsage,
                totalRevenueCents = totalRevenue,
            });
        });
    }
}