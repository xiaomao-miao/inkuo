using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using Microsoft.IdentityModel.Tokens;
using Inkuso.Cloud.Core.Entities;
using Microsoft.Extensions.Configuration;

namespace Inkuso.Cloud.Admin.Auth;

/// <summary>
/// Issues short-lived JWTs for the inkuo Cloud admin panel.
/// Uses a separate JWT secret + audience from the customer JWT so admin
/// tokens cannot be used to call the desktop-facing API.
/// </summary>
public class AdminJwtService(IConfiguration config)
{
    // Minimum key length for HS256: 256 bits = 32 bytes.
    private const int MinSecretLength = 32;

    // Verify the secret once at construction — before any token can be issued.
    private readonly string _secret = ValidateSecret(config);
    private readonly string _issuer = config["Jwt:Issuer"] ?? "inkuo-cloud";
    private readonly string _audience = config["Jwt:AdminAudience"] ?? "inkuo-admin";
    private readonly int _expiryHours = config.GetValue("Jwt:AdminExpiryHours", 12);

    private static string ValidateSecret(IConfiguration config)
    {
        var secret = config["Jwt:Secret"]
            ?? throw new InvalidOperationException("Jwt:Secret is required");
        if (secret.Length < MinSecretLength)
            throw new InvalidOperationException(
                $"Jwt:Secret must be at least {MinSecretLength} characters for HS256. "
              + "Generate one with `openssl rand -base64 48`.");
        if (secret.StartsWith("change-me",    StringComparison.OrdinalIgnoreCase)
         || secret.StartsWith("replace-with", StringComparison.OrdinalIgnoreCase)
         || secret.StartsWith("replace-",     StringComparison.OrdinalIgnoreCase)
         || secret.StartsWith("your-",        StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException(
                "Jwt:Secret must not be a placeholder value.");
        return secret;
    }

    public string GenerateToken(AdminUser user)
    {
        var key = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(_secret));
        var creds = new SigningCredentials(key, SecurityAlgorithms.HmacSha256);

        var now = DateTime.UtcNow;
        var claims = new[]
        {
            new Claim(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new Claim(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString("N")),
            new Claim(JwtRegisteredClaimNames.Iat,
                ((DateTimeOffset)now).ToUnixTimeSeconds().ToString(),
                ClaimValueTypes.Integer64),
            new Claim(ClaimTypes.Name, user.Username),
            new Claim(ClaimTypes.Role, user.Role),
            new Claim("type", "admin"),
        };

        var token = new JwtSecurityToken(
            issuer: _issuer,
            audience: _audience,
            claims: claims,
            notBefore: now,
            expires: now.AddHours(_expiryHours),
            signingCredentials: creds
        );

        return new JwtSecurityTokenHandler().WriteToken(token);
    }

    public int ExpiryHours => _expiryHours;
}
