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
    private readonly string _secret = config["Jwt:Secret"]
        ?? throw new InvalidOperationException("Jwt:Secret is required");
    private readonly string _issuer = config["Jwt:Issuer"] ?? "inkuo-cloud";
    private readonly string _audience = config["Jwt:AdminAudience"] ?? "inkuo-admin";
    private readonly int _expiryHours = config.GetValue("Jwt:AdminExpiryHours", 12);

    public string GenerateToken(AdminUser user)
    {
        var key = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(_secret));
        var creds = new SigningCredentials(key, SecurityAlgorithms.HmacSha256);

        var claims = new[]
        {
            new Claim(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new Claim(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString()),
            new Claim(ClaimTypes.Name, user.Username),
            new Claim(ClaimTypes.Role, user.Role),
            new Claim("type", "admin"),
        };

        var token = new JwtSecurityToken(
            issuer: _issuer,
            audience: _audience,
            claims: claims,
            expires: DateTime.UtcNow.AddHours(_expiryHours),
            signingCredentials: creds
        );

        return new JwtSecurityTokenHandler().WriteToken(token);
    }

    public int ExpiryHours => _expiryHours;
}