using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminInviteCodesEndpoints
{
    public record InviteCodeRequest(
        string Code,
        long FreePoints,
        int MaxUses,
        DateTime? ExpiresAt,
        bool Enabled);

    public static void MapAdminInviteCodesEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/invite-codes").WithTags("invite-codes").RequireAuthorization();

        group.MapGet("/", async (AppDbContext db, int page = 1, int pageSize = 20) =>
        {
            page = Math.Max(1, page);
            pageSize = Math.Clamp(pageSize, 1, 100);

            var query = db.InviteCodes.OrderByDescending(i => i.CreatedAt);
            var total = await query.CountAsync();
            var rows = await query.Skip((page - 1) * pageSize).Take(pageSize).ToListAsync();

            return Results.Ok(new { total, page, pageSize, items = rows });
        });

        group.MapPost("/", async (InviteCodeRequest req, AppDbContext db) =>
        {
            var code = BillingLimits.NormalizeCode(req.Code);
            var validationError = BillingLimits.ValidateCode(code)
                                  ?? BillingLimits.ValidatePointGrant(req.FreePoints, allowZero: true)
                                  ?? BillingLimits.ValidateMaxUses(req.MaxUses);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });

            if (await db.InviteCodes.AnyAsync(i => i.Code == code))
                return Results.Conflict(new { error = "Code already exists" });

            var invite = new InviteCode
            {
                Code = code,
                FreePoints = req.FreePoints,
                MaxUses = req.MaxUses,
                ExpiresAt = req.ExpiresAt,
                Enabled = req.Enabled,
                CreatedAt = DateTime.UtcNow,
            };
            db.InviteCodes.Add(invite);
            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateException)
            {
                return Results.Conflict(new { error = "Code already exists" });
            }
            return Results.Ok(new { id = invite.Id, code = invite.Code });
        });

        group.MapPut("/{id:int}", async (int id, InviteCodeRequest req, AppDbContext db) =>
        {
            var code = BillingLimits.NormalizeCode(req.Code);
            var validationError = BillingLimits.ValidateCode(code)
                                  ?? BillingLimits.ValidatePointGrant(req.FreePoints, allowZero: true)
                                  ?? BillingLimits.ValidateMaxUses(req.MaxUses);
            if (validationError is not null)
                return Results.BadRequest(new { error = validationError });

            var invite = await db.InviteCodes.FindAsync(id);
            if (invite == null) return Results.NotFound();
            if (req.MaxUses < invite.UsedCount)
                return Results.BadRequest(new { error = "MaxUses cannot be lower than UsedCount" });
            if (await db.InviteCodes.AnyAsync(i => i.Id != id && i.Code == code))
                return Results.Conflict(new { error = "Code already exists" });

            invite.Code = code;
            invite.FreePoints = req.FreePoints;
            invite.MaxUses = req.MaxUses;
            invite.ExpiresAt = req.ExpiresAt;
            invite.Enabled = req.Enabled;

            try
            {
                await db.SaveChangesAsync();
            }
            catch (DbUpdateException)
            {
                return Results.Conflict(new { error = "Code already exists" });
            }
            return Results.Ok(new { message = "Invite code updated" });
        });

        group.MapPost("/{id:int}/toggle", async (int id, AppDbContext db) =>
        {
            var invite = await db.InviteCodes.FindAsync(id);
            if (invite == null) return Results.NotFound();
            if (!invite.Enabled)
            {
                var normalizedCode = BillingLimits.NormalizeCode(invite.Code);
                var validationError = normalizedCode != invite.Code
                    ? "Code must not contain leading or trailing whitespace"
                    : BillingLimits.ValidateCode(normalizedCode)
                      ?? BillingLimits.ValidatePointGrant(invite.FreePoints, allowZero: true)
                      ?? BillingLimits.ValidateMaxUses(invite.MaxUses);
                if (validationError is not null || invite.UsedCount > invite.MaxUses)
                    return Results.BadRequest(new
                    {
                        error = validationError ?? "UsedCount exceeds MaxUses; edit the code before enabling it",
                    });
            }
            invite.Enabled = !invite.Enabled;
            await db.SaveChangesAsync();
            return Results.Ok(new { enabled = invite.Enabled });
        });

        group.MapDelete("/{id:int}", async (int id, AppDbContext db) =>
        {
            var invite = await db.InviteCodes.FindAsync(id);
            if (invite == null) return Results.NotFound();
            if (invite.UsedCount > 0)
                return Results.BadRequest(new { error = "Cannot delete invite code that has been used; disable it instead" });
            db.InviteCodes.Remove(invite);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Invite code deleted" });
        });
    }
}
