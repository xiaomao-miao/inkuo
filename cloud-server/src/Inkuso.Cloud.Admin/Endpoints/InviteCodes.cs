using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminInviteCodesEndpoints
{
    public record InviteCodeRequest(
        string Code,
        decimal FreeQuotaCents,
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
            if (string.IsNullOrWhiteSpace(req.Code) || req.Code.Length < 4)
                return Results.BadRequest(new { error = "Code must be at least 4 characters" });

            if (await db.InviteCodes.AnyAsync(i => i.Code == req.Code))
                return Results.Conflict(new { error = "Code already exists" });

            var invite = new InviteCode
            {
                Code = req.Code,
                FreeQuotaCents = req.FreeQuotaCents,
                MaxUses = req.MaxUses,
                ExpiresAt = req.ExpiresAt,
                Enabled = req.Enabled,
                CreatedAt = DateTime.UtcNow,
            };
            db.InviteCodes.Add(invite);
            await db.SaveChangesAsync();
            return Results.Ok(new { id = invite.Id, code = invite.Code });
        });

        group.MapPut("/{id:int}", async (int id, InviteCodeRequest req, AppDbContext db) =>
        {
            var invite = await db.InviteCodes.FindAsync(id);
            if (invite == null) return Results.NotFound();

            invite.Code = req.Code;
            invite.FreeQuotaCents = req.FreeQuotaCents;
            invite.MaxUses = req.MaxUses;
            invite.ExpiresAt = req.ExpiresAt;
            invite.Enabled = req.Enabled;

            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Invite code updated" });
        });

        group.MapPost("/{id:int}/toggle", async (int id, AppDbContext db) =>
        {
            var invite = await db.InviteCodes.FindAsync(id);
            if (invite == null) return Results.NotFound();
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