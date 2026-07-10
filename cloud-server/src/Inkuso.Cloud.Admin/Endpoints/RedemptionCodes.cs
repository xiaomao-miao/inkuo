using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminRedemptionCodesEndpoints
{
    public record RedemptionCodeRequest(
        string Code,
        decimal CreditCents,
        Guid? PlanId,
        int MaxUses,
        DateTime? ExpiresAt,
        bool Enabled);

    public static void MapAdminRedemptionCodesEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/redemption-codes").WithTags("redemption-codes").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db, int page = 1, int pageSize = 20) =>
        {
            page = Math.Max(1, page);
            pageSize = Math.Clamp(pageSize, 1, 100);

            var rows = await db.RedemptionCodes
                .Include(r => r.Plan)
                .OrderByDescending(r => r.CreatedAt)
                .Skip((page - 1) * pageSize)
                .Take(pageSize)
                .Select(r => new
                {
                    r.Id, r.Code, r.CreditCents, r.PlanId,
                    PlanName = r.Plan != null ? r.Plan.Name : null,
                    r.MaxUses, r.UsedCount, r.ExpiresAt, r.CreatedAt, r.Enabled
                })
                .ToListAsync();

            var total = await db.RedemptionCodes.CountAsync();
            return Results.Ok(new { total, page, pageSize, items = rows });
        });

        group.MapPost("/", async (RedemptionCodeRequest req, AppDbContext db) =>
        {
            if (string.IsNullOrWhiteSpace(req.Code) || req.Code.Length < 4)
                return Results.BadRequest(new { error = "Code must be at least 4 characters" });

            if (await db.RedemptionCodes.AnyAsync(r => r.Code == req.Code))
                return Results.Conflict(new { error = "Code already exists" });

            if (req.PlanId.HasValue && !await db.Plans.AnyAsync(p => p.Id == req.PlanId.Value))
                return Results.BadRequest(new { error = "Invalid PlanId" });

            var r = new RedemptionCode
            {
                Code = req.Code,
                CreditCents = req.CreditCents,
                PlanId = req.PlanId,
                MaxUses = req.MaxUses,
                ExpiresAt = req.ExpiresAt,
                Enabled = req.Enabled,
                CreatedAt = DateTime.UtcNow,
            };
            db.RedemptionCodes.Add(r);
            await db.SaveChangesAsync();
            return Results.Ok(new { id = r.Id, code = r.Code });
        });

        group.MapPut("/{id:int}", async (int id, RedemptionCodeRequest req, AppDbContext db) =>
        {
            var r = await db.RedemptionCodes.FindAsync(id);
            if (r == null) return Results.NotFound();

            r.Code = req.Code;
            r.CreditCents = req.CreditCents;
            r.PlanId = req.PlanId;
            r.MaxUses = req.MaxUses;
            r.ExpiresAt = req.ExpiresAt;
            r.Enabled = req.Enabled;

            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Redemption code updated" });
        });

        group.MapPost("/{id:int}/toggle", async (int id, AppDbContext db) =>
        {
            var r = await db.RedemptionCodes.FindAsync(id);
            if (r == null) return Results.NotFound();
            r.Enabled = !r.Enabled;
            await db.SaveChangesAsync();
            return Results.Ok(new { enabled = r.Enabled });
        });

        group.MapDelete("/{id:int}", async (int id, AppDbContext db) =>
        {
            var r = await db.RedemptionCodes.FindAsync(id);
            if (r == null) return Results.NotFound();
            if (r.UsedCount > 0)
                return Results.BadRequest(new { error = "Cannot delete redemption code that has been used; disable it instead" });
            db.RedemptionCodes.Remove(r);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Redemption code deleted" });
        });
    }
}