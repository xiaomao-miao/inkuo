using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminPlansEndpoints
{
    public record PlanRequest(
        string Name,
        long MonthlyPricePoints,
        long MonthlyTokenLimit,
        decimal OverageInputPricePer1k,
        decimal OverageOutputPricePer1k,
        bool Enabled);

    public static void MapAdminPlansEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/plans").WithTags("plans").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db) =>
        {
            var plans = await db.Plans
                .OrderBy(p => p.MonthlyPricePoints)
                .Select(p => new
                {
                    p.Id,
                    p.Name,
                    p.MonthlyPricePoints,
                    p.MonthlyTokenLimit,
                    p.OverageInputPricePer1k,
                    p.OverageOutputPricePer1k,
                    p.Enabled,
                    p.CreatedAt,
                    SubscriberCount = db.Subscriptions.Count(s => s.PlanId == p.Id && s.Status == "active"),
                })
                .ToListAsync();
            return Results.Ok(plans);
        });

        group.MapPost("/", async (PlanRequest req, AppDbContext db) =>
        {
            if (string.IsNullOrWhiteSpace(req.Name))
                return Results.BadRequest(new { error = "Name is required" });
            if (await db.Plans.AnyAsync(p => p.Name == req.Name))
                return Results.Conflict(new { error = "Plan name already exists" });

            var plan = new Plan
            {
                Name = req.Name,
                MonthlyPricePoints = req.MonthlyPricePoints,
                MonthlyTokenLimit = req.MonthlyTokenLimit,
                OverageInputPricePer1k = req.OverageInputPricePer1k,
                OverageOutputPricePer1k = req.OverageOutputPricePer1k,
                Enabled = req.Enabled,
                CreatedAt = DateTime.UtcNow,
            };
            db.Plans.Add(plan);
            await db.SaveChangesAsync();
            return Results.Ok(new { id = plan.Id });
        });

        group.MapPut("/{id:guid}", async (Guid id, PlanRequest req, AppDbContext db) =>
        {
            var plan = await db.Plans.FindAsync(id);
            if (plan == null) return Results.NotFound();

            plan.Name = req.Name;
            plan.MonthlyPricePoints = req.MonthlyPricePoints;
            plan.MonthlyTokenLimit = req.MonthlyTokenLimit;
            plan.OverageInputPricePer1k = req.OverageInputPricePer1k;
            plan.OverageOutputPricePer1k = req.OverageOutputPricePer1k;
            plan.Enabled = req.Enabled;

            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Plan updated" });
        });

        group.MapDelete("/{id:guid}", async (Guid id, AppDbContext db) =>
        {
            var plan = await db.Plans.FindAsync(id);
            if (plan == null) return Results.NotFound();
            var hasSubs = await db.Subscriptions.AnyAsync(s => s.PlanId == id);
            if (hasSubs)
                return Results.BadRequest(new { error = "Cannot delete plan with active subscriptions; disable it instead" });
            db.Plans.Remove(plan);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Plan deleted" });
        });
    }
}