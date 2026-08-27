using System.Security.Claims;
using System.Text.RegularExpressions;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Authorization;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Auth;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Security;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Auth
{
    public record RegisterRequest(string InviteCode, string Email, string Password);
    public record LoginRequest(string Email, string Password);
    public record RefreshRequest(string RefreshToken);
    public record AuthResponse(string AccessToken, string RefreshToken, DateTime ExpiresAt, UserDto User);
    public record UserDto(
        Guid Id,
        string Email,
        long BalancePoints,
        long DebtPoints,
        bool IsSuspended,
        string? PlanName,
        DateTime? SubscriptionExpiresAt);

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

        group.MapPost("/register", async (
            RegisterRequest req,
            AppDbContext db,
            JwtService jwt,
            CancellationToken ct) =>
        {
            // Normalize once so every subsequent lookup is consistent —
            // the previous version validated `req.Email` (raw casing) but
            // stored `req.Email.ToLowerInvariant().Trim()`, letting
            // `Alice@x.com` and `alice@x.com` slip past the duplicate check.
            var normalizedEmail = (req.Email ?? string.Empty).ToLowerInvariant().Trim();

            if (string.IsNullOrEmpty(normalizedEmail)
                || normalizedEmail.Length > 320
                || !EmailRegex.IsMatch(normalizedEmail))
                return Results.BadRequest(new { error = "Invalid email" });
            var passwordError = CredentialPolicy.ValidatePassword(req.Password);
            if (passwordError is not null)
                return Results.BadRequest(new { error = passwordError });

            // Hash before opening the transaction: BCrypt is deliberately slow,
            // and keeping a database transaction open during it would amplify
            // lock contention under registration bursts.
            var passwordHash = BCrypt.Net.BCrypt.HashPassword(req.Password);

            // Cheap pre-check for the common duplicate case; the unique index is
            // still the source of truth for concurrent registrations.
            if (await db.Users.AnyAsync(u => u.Email == normalizedEmail, ct))
                return Results.Conflict(new { error = "Email already registered" });

            var inviteCode = string.IsNullOrWhiteSpace(req.InviteCode)
                ? null
                : req.InviteCode.Trim();
            var freeCredit = 0L;
            var user = new User
            {
                Email = normalizedEmail,
                PasswordHash = passwordHash,
                InviteCodeUsed = inviteCode,
            };

            // Invite-use reservation and user creation form one transaction.
            // Otherwise a duplicate-email race or database failure consumes a
            // limited invite even though no account was created.
            await using var tx = await db.Database.BeginTransactionAsync(ct);
            try
            {
                if (inviteCode is not null)
                {
                    var invite = await db.InviteCodes.AsNoTracking()
                        .FirstOrDefaultAsync(i =>
                            i.Code == inviteCode
                            && i.Enabled
                            && i.UsedCount < i.MaxUses
                            && (i.ExpiresAt == null || i.ExpiresAt > DateTime.UtcNow), ct);
                    if (invite is null)
                    {
                        await tx.RollbackAsync(ct);
                        return Results.BadRequest(new { error = "Invalid, expired, or exhausted invite code" });
                    }
                    if (BillingLimits.ValidatePointGrant(invite.FreePoints, allowZero: true) is not null)
                    {
                        await tx.RollbackAsync(ct);
                        return Results.Json(
                            new { error = "Invite code is temporarily unavailable; contact support." },
                            statusCode: 503);
                    }

                    var reserved = await db.InviteCodes
                        .Where(i => i.Id == invite.Id
                                    && i.Enabled
                                    && i.UsedCount < i.MaxUses
                                    && (i.ExpiresAt == null || i.ExpiresAt > DateTime.UtcNow))
                        .ExecuteUpdateAsync(s => s.SetProperty(
                            i => i.UsedCount,
                            i => i.UsedCount + 1), ct);
                    if (reserved != 1)
                    {
                        await tx.RollbackAsync(ct);
                        return Results.BadRequest(new { error = "Invite code has reached its usage limit" });
                    }
                    freeCredit = invite.FreePoints;
                }

                user.BalancePoints = freeCredit;
                db.Users.Add(user);
                await db.SaveChangesAsync(ct);
                await tx.CommitAsync(ct);
            }
            catch (Microsoft.EntityFrameworkCore.DbUpdateException)
            {
                await tx.RollbackAsync(CancellationToken.None);
                db.ChangeTracker.Clear();
                if (await db.Users.AsNoTracking().AnyAsync(
                        u => u.Email == normalizedEmail, CancellationToken.None))
                    return Results.Conflict(new { error = "Email already registered" });
                throw;
            }
            catch
            {
                await tx.RollbackAsync(CancellationToken.None);
                throw;
            }

            var tokens = await jwt.GenerateTokensAsync(user);
            var sub = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => s.UserId == user.Id
                            && s.Status == "active"
                            && s.ExpiresAt > DateTime.UtcNow)
                .OrderByDescending(s => s.ExpiresAt)
                .FirstOrDefaultAsync();

            return Results.Ok(new AuthResponse(
                tokens.AccessToken,
                tokens.RefreshToken,
                tokens.AccessExpiresAt,
                new UserDto(user.Id, user.Email, user.BalancePoints, user.DebtPoints, user.IsSuspended,
                    sub?.Plan.Name, sub?.ExpiresAt)
            ));
        });

        group.MapPost("/login", async (LoginRequest req, AppDbContext db, JwtService jwt) =>
        {
            var normalizedEmail = (req.Email ?? string.Empty).ToLowerInvariant().Trim();
            if (normalizedEmail.Length is 0 or > 320 || string.IsNullOrEmpty(req.Password))
                return Results.Unauthorized();
            var user = await db.Users.FirstOrDefaultAsync(u => u.Email == normalizedEmail);
            if (user == null || !BCrypt.Net.BCrypt.Verify(req.Password, user.PasswordHash))
                return Results.Unauthorized();

            var tokens = await jwt.GenerateTokensAsync(user);
            var sub = await db.Subscriptions
                .Include(s => s.Plan)
                .Where(s => s.UserId == user.Id
                            && s.Status == "active"
                            && s.ExpiresAt > DateTime.UtcNow)
                .OrderByDescending(s => s.ExpiresAt)
                .FirstOrDefaultAsync();

            return Results.Ok(new AuthResponse(
                tokens.AccessToken,
                tokens.RefreshToken,
                tokens.AccessExpiresAt,
                new UserDto(user.Id, user.Email, user.BalancePoints, user.DebtPoints, user.IsSuspended,
                    sub?.Plan.Name, sub?.ExpiresAt)
            ));
        });

        group.MapPost("/refresh", async (RefreshRequest req, JwtService jwt) =>
        {
            // `db` is no longer injected here — JwtService owns its own
            // DbContext, and pulling the unused service in was producing
            // a "you don't need this" analyzer warning.
            if (string.IsNullOrWhiteSpace(req.RefreshToken)) return Results.Unauthorized();
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
            if (!Guid.TryParse(userId, out var parsedUserId)) return Results.Unauthorized();
            await jwt.RevokeAllUserTokensAsync(parsedUserId);
            return Results.Ok();
        });
    }
}
