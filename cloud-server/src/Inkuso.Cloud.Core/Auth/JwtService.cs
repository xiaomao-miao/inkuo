using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Security.Cryptography;
using System.Text;
using Microsoft.IdentityModel.Tokens;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Data;
using Microsoft.EntityFrameworkCore;

namespace Inkuso.Cloud.Core.Auth;

public class JwtService(AppDbContext db, JwtSettings settings)
{
    /// <summary>
    /// Access + refresh pair issued at login / register.
    /// </summary>
    public record Tokens(string AccessToken, string RefreshToken, DateTime AccessExpiresAt);

    /// <summary>
    /// Refresh response. <see cref="Succeeded"/> is true when the supplied
    /// refresh token was valid and was rotated; <see cref="NewRefreshToken"/>
    /// is null on failure (caller should treat as 401).
    /// </summary>
    public record RefreshResult(string AccessToken, string? NewRefreshToken, DateTime AccessExpiresAt, bool Succeeded);

    public string GenerateAccessToken(User user)
    {
        var key = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(settings.Secret));
        var creds = new SigningCredentials(key, SecurityAlgorithms.HmacSha256);

        var now = DateTime.UtcNow;
        var claims = new[]
        {
            new Claim(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new Claim(JwtRegisteredClaimNames.Email, user.Email),
            new Claim(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString("N")),
            new Claim(JwtRegisteredClaimNames.Iat,
                ((DateTimeOffset)now).ToUnixTimeSeconds().ToString(),
                ClaimValueTypes.Integer64),
            new Claim("type", "access"),
        };

        var token = new JwtSecurityToken(
            issuer: settings.Issuer,
            audience: settings.Audience,
            claims: claims,
            notBefore: now,
            expires: now.AddMinutes(settings.AccessExpiryMinutes),
            signingCredentials: creds
        );

        return new JwtSecurityTokenHandler().WriteToken(token);
    }

    public async Task<Tokens> GenerateTokensAsync(User user)
    {
        var accessToken = GenerateAccessToken(user);
        var refreshToken = await CreateRefreshTokenAsync(user);
        var expiresAt = DateTime.UtcNow.AddMinutes(settings.AccessExpiryMinutes);
        return new Tokens(accessToken, refreshToken, expiresAt);
    }

    /// <summary>
    /// Persist a new refresh token for the user and return its opaque id.
    /// Uses <see cref="RandomNumberGenerator"/> (CSPRNG) for the secret and
    /// base64url encoding so the value is safe to put in the JWT 'sub' field
    /// and inside HTTP headers without escaping.
    /// </summary>
    public async Task<string> CreateRefreshTokenAsync(User user)
    {
        var token = CreateRefreshToken(user.Id);

        db.RefreshTokens.Add(token);
        await db.SaveChangesAsync();
        return token.Jti;
    }

    /// <summary>
    /// Rotate the supplied refresh token: revoke the old one and issue a
    /// fresh access + refresh pair. Returns <see cref="RefreshResult.Succeeded"/>=false
    /// when the supplied token is unknown / revoked / expired — caller
    /// should respond 401 and force the client to log in again.
    /// </summary>
    public async Task<RefreshResult> RefreshAccessTokenAsync(string refreshTokenJti)
    {
        if (db.Database.IsRelational())
            return await RefreshRelationalAsync(refreshTokenJti);

        // Look up the refresh token. We deliberately do NOT `Include` the
        // user navigation here: EF Core 10's InMemory provider (used in
        // unit tests) returns zero rows when the navigation's principal
        // isn't tracked, while Postgres handles Include normally. We load
        // the user in a second query below, which is also clearer in
        // Postgres EXPLAIN plans.
        var rt = await db.RefreshTokens
            .FirstOrDefaultAsync(r =>
                r.Jti == refreshTokenJti &&
                !r.Revoked &&
                r.ExpiresAt > DateTime.UtcNow);

        if (rt is null)
        {
            return new RefreshResult(string.Empty, null, DateTime.MinValue, false);
        }

        var user = await db.Users.FindAsync(rt.UserId);
        if (user is null)
        {
            // Stale refresh token pointing at a deleted user — treat as 401
            // and let the client re-authenticate.
            return new RefreshResult(string.Empty, null, DateTime.MinValue, false);
        }

        // The in-memory provider used by unit tests has no relational
        // ExecuteUpdate support. One SaveChanges still keeps this path atomic.
        rt.Revoked = true;
        var replacement = CreateRefreshToken(user.Id);
        db.RefreshTokens.Add(replacement);
        await db.SaveChangesAsync();

        return SuccessfulRefresh(user, replacement.Jti);
    }

    private async Task<RefreshResult> RefreshRelationalAsync(string refreshTokenJti)
    {
        await using var tx = await db.Database.BeginTransactionAsync();
        try
        {
            var now = DateTime.UtcNow;
            var token = await db.RefreshTokens.AsNoTracking()
                .Where(r => r.Jti == refreshTokenJti && !r.Revoked && r.ExpiresAt > now)
                .Select(r => new { r.Id, r.UserId })
                .FirstOrDefaultAsync();
            if (token is null)
                return FailedRefresh();

            // Claim the old token with one conditional UPDATE. PostgreSQL
            // re-checks the predicate after a concurrent row lock is released,
            // so exactly one of two simultaneous refresh requests can affect
            // the row; the loser receives a normal authentication failure.
            var claimed = await db.RefreshTokens
                .Where(r => r.Id == token.Id && !r.Revoked && r.ExpiresAt > now)
                .ExecuteUpdateAsync(setters => setters.SetProperty(r => r.Revoked, true));
            if (claimed != 1)
            {
                await tx.RollbackAsync();
                return FailedRefresh();
            }

            var user = await db.Users.FindAsync(token.UserId);
            if (user is null)
            {
                await tx.RollbackAsync();
                return FailedRefresh();
            }

            var replacement = CreateRefreshToken(user.Id);
            db.RefreshTokens.Add(replacement);
            await db.SaveChangesAsync();
            await tx.CommitAsync();
            return SuccessfulRefresh(user, replacement.Jti);
        }
        catch
        {
            await tx.RollbackAsync();
            throw;
        }
    }

    public async Task RevokeAllUserTokensAsync(Guid userId)
    {
        var tokens = await db.RefreshTokens.Where(rt => rt.UserId == userId && !rt.Revoked).ToListAsync();
        if (tokens.Count == 0) return;
        foreach (var t in tokens) t.Revoked = true;
        await db.SaveChangesAsync();
    }

    private RefreshToken CreateRefreshToken(Guid userId) => new()
    {
        Jti = Base64UrlEncode(RandomNumberGenerator.GetBytes(32)),
        UserId = userId,
        ExpiresAt = DateTime.UtcNow.AddDays(settings.RefreshExpiryDays),
    };

    private RefreshResult SuccessfulRefresh(User user, string refreshToken) => new(
        GenerateAccessToken(user),
        refreshToken,
        DateTime.UtcNow.AddMinutes(settings.AccessExpiryMinutes),
        true);

    private static RefreshResult FailedRefresh() =>
        new(string.Empty, null, DateTime.MinValue, false);

    private static string Base64UrlEncode(byte[] bytes) =>
        Convert.ToBase64String(bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_');
}

public class JwtSettings
{
    public string Secret { get; set; } = string.Empty;
    public string Issuer { get; set; } = "inkuo-cloud";
    public string Audience { get; set; } = "inkuo-desktop";
    public int AccessExpiryMinutes { get; set; } = 15;
    public int RefreshExpiryDays { get; set; } = 30;
}
