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
    public record Tokens(string AccessToken, string RefreshToken, DateTime AccessExpiresAt);

    public record RefreshResult(string AccessToken, string? NewRefreshToken, DateTime AccessExpiresAt, bool Invalidated);

    public string GenerateAccessToken(User user)
    {
        var key = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(settings.Secret));
        var creds = new SigningCredentials(key, SecurityAlgorithms.HmacSha256);

        var claims = new[]
        {
            new Claim(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new Claim(JwtRegisteredClaimNames.Email, user.Email),
            new Claim(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString()),
            new Claim("type", "access"),
        };

        var token = new JwtSecurityToken(
            issuer: settings.Issuer,
            audience: settings.Audience,
            claims: claims,
            expires: DateTime.UtcNow.AddMinutes(settings.AccessExpiryMinutes),
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

    public async Task<string> CreateRefreshTokenAsync(User user)
    {
        var token = new RefreshToken
        {
            Jti = Convert.ToBase64String(RandomNumberGenerator.GetBytes(32)),
            UserId = user.Id,
            ExpiresAt = DateTime.UtcNow.AddDays(settings.RefreshExpiryDays),
        };

        db.RefreshTokens.Add(token);
        await db.SaveChangesAsync();
        return token.Jti;
    }

    public async Task<RefreshResult> RefreshAccessTokenAsync(string refreshTokenJti)
    {
        var rt = await db.RefreshTokens
            .Include(r => r.User)
            .FirstOrDefaultAsync(r => r.Jti == refreshTokenJti && !r.Revoked && r.ExpiresAt > DateTime.UtcNow);

        if (rt == null)
            return new RefreshResult("", null, DateTime.MinValue, false);

        // Revoke old refresh token
        rt.Revoked = true;

        var accessToken = GenerateAccessToken(rt.User);
        var newRefreshToken = await CreateRefreshTokenAsync(rt.User);
        await db.SaveChangesAsync();

        return new RefreshResult(accessToken, newRefreshToken, DateTime.UtcNow.AddMinutes(settings.AccessExpiryMinutes), true);
    }

    public async Task RevokeAllUserTokensAsync(Guid userId)
    {
        var tokens = await db.RefreshTokens.Where(rt => rt.UserId == userId && !rt.Revoked).ToListAsync();
        foreach (var t in tokens) t.Revoked = true;
        await db.SaveChangesAsync();
    }
}

public class JwtSettings
{
    public string Secret { get; set; } = string.Empty;
    public string Issuer { get; set; } = "inkuo-cloud";
    public string Audience { get; set; } = "inkuo-desktop";
    public int AccessExpiryMinutes { get; set; } = 15;
    public int RefreshExpiryDays { get; set; } = 30;
}
