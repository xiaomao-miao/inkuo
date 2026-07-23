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
        var token = new RefreshToken
        {
            Jti = Base64UrlEncode(RandomNumberGenerator.GetBytes(32)),
            UserId = user.Id,
            ExpiresAt = DateTime.UtcNow.AddDays(settings.RefreshExpiryDays),
        };

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

        // Rotate inside a single transaction so a crash between "revoke old"
        // and "issue new" can never leave the user with no valid refresh
        // token (the previous implementation called SaveChanges twice).
        // We tolerate the in-memory provider's lack of cross-store transactions
        // (used by tests) by skipping the explicit begin/commit — SaveChanges
        // remains atomic against a single provider.
        var supportsTx = db.Database.IsRelational();
        Microsoft.EntityFrameworkCore.Storage.IDbContextTransaction? tx = null;
        if (supportsTx) tx = await db.Database.BeginTransactionAsync();
        try
        {
            rt.Revoked = true;
            var accessToken = GenerateAccessToken(user);
            var newRefreshToken = await CreateRefreshTokenAsync(user);
            await db.SaveChangesAsync();
            if (tx is not null) await tx.CommitAsync();

            return new RefreshResult(
                accessToken,
                newRefreshToken,
                DateTime.UtcNow.AddMinutes(settings.AccessExpiryMinutes),
                true);
        }
        catch
        {
            if (tx is not null) await tx.RollbackAsync();
            throw;
        }
        finally
        {
            if (tx is not null) await tx.DisposeAsync();
        }
    }

    public async Task RevokeAllUserTokensAsync(Guid userId)
    {
        var tokens = await db.RefreshTokens.Where(rt => rt.UserId == userId && !rt.Revoked).ToListAsync();
        if (tokens.Count == 0) return;
        foreach (var t in tokens) t.Revoked = true;
        await db.SaveChangesAsync();
    }

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
