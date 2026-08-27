using System.Security.Claims;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Admin.Auth;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Security;

namespace Inkuso.Cloud.Admin.Endpoints;

public static class AdminAuthEndpoints
{
    public record LoginRequest(string Username, string Password);
    public record LoginResponse(string AccessToken, DateTime ExpiresAt, AdminDto Admin);
    public record AdminDto(Guid Id, string Username, string Role);
    public record ChangePasswordRequest(string CurrentPassword, string NewPassword);
    public record CreateAdminRequest(string Username, string Password, string Role);

    private static string? RoleOf(HttpContext ctx) =>
        ctx.User.FindFirst(ClaimTypes.Role)?.Value;

    public static void MapAdminAuthEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/api/auth").WithTags("admin-auth");

        group.MapPost("/login", async (LoginRequest req, AppDbContext db, AdminJwtService jwt) =>
        {
            if (string.IsNullOrWhiteSpace(req.Username) || string.IsNullOrWhiteSpace(req.Password))
                return Results.BadRequest(new { error = "Username and password required" });

            var username = req.Username.Trim();
            if (CredentialPolicy.ValidateAdminUsername(username) is not null)
                return Results.BadRequest(new { error = "Invalid username or password" });
            var user = await db.AdminUsers.FirstOrDefaultAsync(u => u.Username == username);
            if (user == null || !user.Enabled || !BCrypt.Net.BCrypt.Verify(req.Password, user.PasswordHash))
                return Results.Unauthorized();

            user.LastLoginAt = DateTime.UtcNow;
            await db.SaveChangesAsync();

            return Results.Ok(new LoginResponse(
                jwt.GenerateToken(user),
                DateTime.UtcNow.AddHours(jwt.ExpiryHours),
                new AdminDto(user.Id, user.Username, user.Role)
            ));
        });

        group.MapGet("/me", [Authorize] async (HttpContext ctx, AppDbContext db) =>
        {
            var id = ctx.User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                     ?? ctx.User.FindFirst("sub")?.Value;
            if (!Guid.TryParse(id, out var adminId)) return Results.Unauthorized();

            var user = await db.AdminUsers.FindAsync(adminId);
            if (user == null || !user.Enabled) return Results.Unauthorized();
            return Results.Ok(new AdminDto(user.Id, user.Username, user.Role));
        });

        group.MapPost("/change-password", [Authorize] async (ChangePasswordRequest req, HttpContext ctx, AppDbContext db) =>
        {
            var id = ctx.User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                     ?? ctx.User.FindFirst("sub")?.Value;
            if (!Guid.TryParse(id, out var adminId)) return Results.Unauthorized();

            var user = await db.AdminUsers.FindAsync(adminId);
            if (user == null) return Results.Unauthorized();
            if (string.IsNullOrEmpty(req.CurrentPassword))
                return Results.BadRequest(new { error = "Current password is required" });
            if (!BCrypt.Net.BCrypt.Verify(req.CurrentPassword, user.PasswordHash))
                return Results.BadRequest(new { error = "Current password is incorrect" });
            var passwordError = CredentialPolicy.ValidatePassword(req.NewPassword);
            if (passwordError is not null)
                return Results.BadRequest(new { error = passwordError });

            user.PasswordHash = BCrypt.Net.BCrypt.HashPassword(req.NewPassword);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Password updated" });
        });

        group.MapPost("/create", [Authorize] async (CreateAdminRequest req, HttpContext ctx, AppDbContext db) =>
        {
            if (RoleOf(ctx) != "superadmin") return Results.Forbid();

            var username = req.Username?.Trim() ?? string.Empty;
            var usernameError = CredentialPolicy.ValidateAdminUsername(username);
            if (usernameError is not null)
                return Results.BadRequest(new { error = usernameError });
            var passwordError = CredentialPolicy.ValidatePassword(req.Password);
            if (passwordError is not null)
                return Results.BadRequest(new { error = passwordError });

            if (await db.AdminUsers.AnyAsync(u => u.Username == username))
                return Results.Conflict(new { error = "Username already exists" });

            db.AdminUsers.Add(new Core.Entities.AdminUser
            {
                Username = username,
                PasswordHash = BCrypt.Net.BCrypt.HashPassword(req.Password),
                Role = req.Role == "superadmin" ? "superadmin" : "admin",
                Enabled = true,
                CreatedAt = DateTime.UtcNow,
            });
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Admin created" });
        });

        group.MapGet("/", [Authorize] async (AppDbContext db) =>
        {
            var admins = await db.AdminUsers
                .OrderBy(u => u.CreatedAt)
                .Select(u => new { u.Id, u.Username, u.Role, u.Enabled, u.CreatedAt, u.LastLoginAt })
                .ToListAsync();
            return Results.Ok(admins);
        });

        group.MapDelete("/{id:guid}", [Authorize] async (Guid id, HttpContext ctx, AppDbContext db) =>
        {
            if (RoleOf(ctx) != "superadmin") return Results.Forbid();

            var myId = ctx.User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                       ?? ctx.User.FindFirst("sub")?.Value;
            if (Guid.TryParse(myId, out var currentAdminId) && currentAdminId == id)
                return Results.BadRequest(new { error = "Cannot delete yourself" });

            var user = await db.AdminUsers.FindAsync(id);
            if (user == null) return Results.NotFound();
            db.AdminUsers.Remove(user);
            await db.SaveChangesAsync();
            return Results.Ok(new { message = "Admin deleted" });
        });
    }
}
