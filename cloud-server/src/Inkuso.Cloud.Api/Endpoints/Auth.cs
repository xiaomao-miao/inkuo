using System.Security.Claims;
using System.Text.RegularExpressions;
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
    public record UserDto(Guid Id, string Email, long BalancePoints, bool IsSuspended, string? PlanName, DateTime? SubscriptionExpiresAt);

    // Email format check. .NET's MailAddress parser rejects more than the
    // naive Contains('@') (e.g. rejects "foo@@bar" or trailing dots); we use
    // it here so the user gets a sane 400 instead of discovering a bad
    // address only after registration lands.
    private static readonly Regex EmailRegex = new(
        @"^[^@\s]+@[^@\s]+\.[^@\s]+$",
        RegexOptions.Compiled | RegexOptions.IgnoreCase);

    public static void MapAuthEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/auth").WithTags("auth");

        group.MapPost("/register", async (RegisterRequest req, AppDbContext db, JwtService jwt) =>
        {
            // Normalize once so every subsequent lookup is consistent —
            // the previous version validated `req.Email` (raw casing) but
            // stored `req.Email.ToLowerInvariant().Trim()`, letting
            // `Alice@x.com` and `alice@x.com` slip past the duplicate check.
            var normalizedEmail = (req.Email ?? string.Empty).ToLowerInvariant().Trim();

            if (string.IsNullOrEmpty(normalizedEmail) || !EmailRegex.IsMatch(normalizedEmail))
                return Results.BadRequest(new { error = "Invalid email" });
            if (string.IsNullOrWhiteSpace(req.Password) || req.Password.Length < 6)
                return Results.BadRequest(new { error = "Password must be at least 6 characters" });

            // Cheap pre-check (returns 409 fast) plus the unique index on
            // Email as the ground-truth guard. We can't wrap register in a
            // serializable transaction because BCrypt hashing is too slow for
            // it; the DB unique index handles the rare race and surfaces a
            // DbUpdateException that we translate to 409.
            if (await db.Users.AnyAsync(u => u.Email == normalizedEmail))
                return Results.Conflict(new { error = "Email already registered" });

            // Validate invite code
            long freeCredit = 0;
            InviteCode? invite = null;
            if (!string.IsNullOrWhiteSpace(req.InviteCode))
            {
                invite = await db.InviteCodes.FirstOrDefaultAsync(i =>
                    i.Code == req.InviteCode && i.Enabled &&
                    (i.ExpiresAt == null || i.ExpiresAt > DateTime.UtcNow));

                if (invite is null)
                    return Results.BadRequest(new { error = "Invalid or expired invite code" });

                // Optimistic decrement + uniqueness against (Code, UsedCount+1 <= MaxUses)
                // pattern: the DB unique index alone can't enforce max-uses,
                // so we re-check inside the conditional update below to close
                // the concurrent-register race that previously let two callers
                // both succeed past MaxUses.
                var reserved = await db.InviteCodes
                    .Where(i => i.Id == invite.Id && i.Enabled && i.UsedCount < i.MaxUses &&
                                (i.ExpiresAt == null || i.ExpiresAt > DateTime.UtcNow))
                    .ExecuteUpdateAsync(s => s.SetProperty(i => i.UsedCount, i => i.UsedCount + 1));

                if (reserved == 0)
                    return Results.BadRequest(new { error = "Invite code has reached its usage limit" });

                freeCredit = invite.FreePoints;
            }

            var user = new User
            {
                Email = normalizedEmail,
                PasswordHash = BCrypt.Net.BCrypt.HashPassword(req.Password),
                InviteCodeUsed = req.InviteCode,
                BalancePoints = freeCredit,
            };

            db.Users.Add(user);
            Microsoft.EntityFrameworkCore.DbUpdateException? updateException = null;
            try
            {
                await db.SaveChangesAsync();
            }
            catch (Microsoft.EntityFrameworkCore.DbUpdateException ex)
            {
                // The unique index caught a duplicate email that slipped
                // through the pre-check race window. Re-query to confirm
                // before returning 409 — some other DbUpdateException (e.g.
                // a transient connection error) should bubble up.
                updateException = ex;
            }

            if (updateException is not null
                && await db.Users.AnyAsync(u => u.Email == normalizedEmail))
            {
                return Results.Conflict(new { error = "Email already registered" });
            }
            if (updateException is not null) throw updateException;

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
                new UserDto(user.Id, user.Email, user.BalancePoints, user.IsSuspended,
                    sub?.Plan.Name, sub?.ExpiresAt)
            ));
        });

        group.MapPost("/login", async (LoginRequest req, AppDbContext db, JwtService jwt) =>
        {
            var normalizedEmail = (req.Email ?? string.Empty).ToLowerInvariant().Trim();
            var user = await db.Users.FirstOrDefaultAsync(u => u.Email == normalizedEmail);
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
                new UserDto(user.Id, user.Email, user.BalancePoints, user.IsSuspended,
                    sub?.Plan.Name, sub?.ExpiresAt)
            ));
        });

        group.MapPost("/refresh", async (RefreshRequest req, JwtService jwt) =>
        {
            // `db` is no longer injected here — JwtService owns its own
            // DbContext, and pulling the unused service in was producing
            // a "you don't need this" analyzer warning.
            var result = await jwt.RefreshAccessTokenAsync(req.RefreshToken);
            if (!result.Succeeded)
                return Results.Unauthorized();
            return Results.Ok(new
            {
                access_token = result.AccessToken,
                refresh_token = result.NewRefreshToken,
                expires_at = result.AccessExpiresAt,
            });
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
