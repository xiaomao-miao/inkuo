using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminRedemptionCodesEndpoints
{
    public record RedemptionCodeRequest(
        string Code,
        long CreditPoints,
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
                    r.Id, r.Code, r.CreditPoints, r.PlanId,
                    PlanName = r.Plan != null ? r.Plan.Name : null,
                    r.MaxUses, r.UsedCount, r.ExpiresAt, r.CreatedAt, r.Enabled
                })
                .ToListAsync();

            var total = await db.RedemptionCodes.CountAsync();
            return Results.Ok(new { total, page, pageSize, items = rows });
        });

        group.MapPost("/", async (RedemptionCodeRequest req, AppDbContext db) =>
        {
            var code = BillingLimits.NormalizeCode(req.Code);
            var validationError = BillingLimits.ValidateCode(code)
                                  ?? BillingLimits.ValidatePointGrant(req.CreditPoints, allowZero: true)
                                  ?? BillingLimits.ValidateMaxUses(req.MaxUses);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });

            if (await db.RedemptionCodes.AnyAsync(r => r.Code == code))
                return Results.Conflict(new { error = "Code already exists" });

            if (req.PlanId.HasValue && !await db.Plans.AnyAsync(p => p.Id == req.PlanId.Value))
                return Results.BadRequest(new { error = "Invalid PlanId" });

            // A code without either points or a plan is useless — refuse to
            // create it so the admin UI doesn't accumulate empty rows.
            if (req.CreditPoints <= 0 && !req.PlanId.HasValue)
                return Results.BadRequest(new { error = "Redemption code must grant points or a plan" });

            var r = new RedemptionCode
            {
                Code = code,
                CreditPoints = req.CreditPoints,
                PlanId = req.PlanId,
                MaxUses = req.MaxUses,
                ExpiresAt = req.ExpiresAt,
                Enabled = req.Enabled,
                CreatedAt = DateTime.UtcNow,
            };
            db.RedemptionCodes.Add(r);
            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateException)
            {
                return Results.Conflict(new { error = "Code already exists" });
            }
            return Results.Ok(new { id = r.Id, code = r.Code });
        });

        group.MapPut("/{id:int}", async (int id, RedemptionCodeRequest req, AppDbContext db) =>
        {
            var code = BillingLimits.NormalizeCode(req.Code);
            var validationError = BillingLimits.ValidateCode(code)
                                  ?? BillingLimits.ValidatePointGrant(req.CreditPoints, allowZero: true)
                                  ?? BillingLimits.ValidateMaxUses(req.MaxUses);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });
            if (req.PlanId.HasValue && !await db.Plans.AnyAsync(p => p.Id == req.PlanId.Value))
                return Results.BadRequest(new { error = "Invalid PlanId" });
            if (req.CreditPoints <= 0 && !req.PlanId.HasValue)
                return Results.BadRequest(new { error = "Redemption code must grant points or a plan" });

            var r = await db.RedemptionCodes.FindAsync(id);
            if (r == null) return Results.NotFound();
            if (req.MaxUses < r.UsedCount)
                return Results.BadRequest(new { error = "MaxUses cannot be lower than UsedCount" });
            if (await db.RedemptionCodes.AnyAsync(other => other.Id != id && other.Code == code))
                return Results.Conflict(new { error = "Code already exists" });

            r.Code = code;
            r.CreditPoints = req.CreditPoints;
            r.PlanId = req.PlanId;
            r.MaxUses = req.MaxUses;
            r.ExpiresAt = req.ExpiresAt;
            r.Enabled = req.Enabled;

            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateException)
            {
                return Results.Conflict(new { error = "Code already exists" });
            }
            return Results.Ok(new { message = "Redemption code updated" });
        });

        group.MapPost("/{id:int}/toggle", async (int id, AppDbContext db) =>
        {
            var r = await db.RedemptionCodes.FindAsync(id);
            if (r == null) return Results.NotFound();
            if (!r.Enabled)
            {
                var normalizedCode = BillingLimits.NormalizeCode(r.Code);
                var validationError = normalizedCode != r.Code
                    ? "Code must not contain leading or trailing whitespace"
                    : BillingLimits.ValidateCode(normalizedCode)
                      ?? BillingLimits.ValidatePointGrant(r.CreditPoints, allowZero: true)
                      ?? BillingLimits.ValidateMaxUses(r.MaxUses);
                if (validationError is not null
                    || r.UsedCount > r.MaxUses
                    || (r.CreditPoints == 0 && !r.PlanId.HasValue)
                    || (r.PlanId.HasValue && !await db.Plans.AnyAsync(plan => plan.Id == r.PlanId.Value)))
                    return Results.BadRequest(new
                    {
                        error = validationError
                                ?? (r.UsedCount > r.MaxUses
                                    ? "UsedCount exceeds MaxUses; edit the code before enabling it"
                                    : "The code must reference an existing plan or grant points"),
                    });
            }
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
