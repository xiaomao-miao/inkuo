using System.Security.Claims;
using System.Text;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Auth;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Auth
{
    public record RegisterRequest(string InviteCode, string Email, string Password);
    public record LoginRequest(string Email, string Password);
    public record RefreshRequest(string RefreshToken);
    public record AuthResponse(string AccessToken, string RefreshToken, DateTime ExpiresAt, UserDto User);
    public record UserDto(Guid Id, string Email, decimal BalanceCents, string? PlanName, DateTime? SubscriptionExpiresAt);

    public static void MapAuthEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/auth").WithTags("auth");

        group.MapPost("/register", async (RegisterRequest req, AppDbContext db, JwtService jwt) =>
        {
            if (string.IsNullOrWhiteSpace(req.Email) || !req.Email.Contains('@'))
                return Results.BadRequest(new { error = "Invalid email" });
            if (string.IsNullOrWhiteSpace(req.Password) || req.Password.Length < 6)
                return Results.BadRequest(new { error = "Password must be at least 6 characters" });

            if (await db.Users.AnyAsync(u => u.Email == req.Email))
                return Results.Conflict(new { error = "Email already registered" });

            // Validate invite code
            decimal freeCredit = 0;
            if (!string.IsNullOrWhiteSpace(req.InviteCode))
            {
                var invite = await db.InviteCodes.FirstOrDefaultAsync(i =>
                    i.Code == req.InviteCode && i.Enabled &&
                    (i.ExpiresAt == null || i.ExpiresAt > DateTime.UtcNow) &&
                    i.UsedCount < i.MaxUses);

                if (invite == null)
                    return Results.BadRequest(new { error = "Invalid or expired invite code" });

                invite.UsedCount++;
                freeCredit = invite.FreeQuotaCents;
            }

            var user = new User
            {
                Email = req.Email.ToLowerInvariant().Trim(),
                PasswordHash = BCrypt.Net.BCrypt.HashPassword(req.Password),
                InviteCodeUsed = req.InviteCode,
                BalanceCents = freeCredit,
            };

            db.Users.Add(user);
            await db.SaveChangesAsync();

            var tokens = await jwt.GenerateTokensAsync(user);
            var sub = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => s.UserId == user.Id && s.Status == "active")
                .OrderByDescending(s => s.ExpiresAt)
                .FirstOrDefaultAsync();

            return Results.Ok(new AuthResponse(
                tokens.AccessToken,
                tokens.RefreshToken,
                tokens.AccessExpiresAt,
                new UserDto(user.Id, user.Email, user.BalanceCents,
                    sub?.Plan.Name, sub?.ExpiresAt)
            ));
        });

        group.MapPost("/login", async (LoginRequest req, AppDbContext db, JwtService jwt) =>
        {
            var user = await db.Users.FirstOrDefaultAsync(u => u.Email == req.Email.ToLowerInvariant().Trim());
            if (user == null || !BCrypt.Net.BCrypt.Verify(req.Password, user.PasswordHash))
                return Results.Unauthorized();

            var tokens = await jwt.GenerateTokensAsync(user);
            var sub = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => s.UserId == user.Id && s.Status == "active")
                .OrderByDescending(s => s.ExpiresAt)
                .FirstOrDefaultAsync();

            return Results.Ok(new AuthResponse(
                tokens.AccessToken,
                tokens.RefreshToken,
                tokens.AccessExpiresAt,
                new UserDto(user.Id, user.Email, user.BalanceCents,
                    sub?.Plan.Name, sub?.ExpiresAt)
            ));
        });

        group.MapPost("/refresh", async (RefreshRequest req, AppDbContext db, JwtService jwt) =>
        {
            var result = await jwt.RefreshAccessTokenAsync(req.RefreshToken);
            if (!result.Invalidated)
                return Results.Unauthorized();
            return Results.Ok(new { access_token = result.AccessToken, refresh_token = result.NewRefreshToken, expires_at = result.AccessExpiresAt });
        });

        group.MapPost("/logout", [Authorize] async (HttpContext ctx, JwtService jwt) =>
        {
            var userId = ctx.User.FindFirst(ClaimTypes.NameIdentifier)?.Value
                         ?? ctx.User.FindFirst("sub")?.Value;
            if (userId == null) return Results.Unauthorized();
            await jwt.RevokeAllUserTokensAsync(Guid.Parse(userId));
            return Results.Ok();
        });
    }
}
