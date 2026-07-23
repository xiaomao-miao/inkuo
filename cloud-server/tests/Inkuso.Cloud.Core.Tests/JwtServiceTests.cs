// <copyright file="JwtServiceTests.cs" company="inkuo">
// Unit tests for JwtService against an in-memory EF Core database. We use
// the InMemory provider so tests are hermetic and don't need Postgres.
//
// Scenarios covered:
//   - GenerateAccessToken emits a JWT with the right claims (sub = user id,
//     email claim, jti, type=access) and a non-empty refresh token
//   - RefreshAccessTokenAsync rotates the refresh token and revokes the old
//     one; the new refresh token is required for subsequent refreshes
//   - RevokeAllUserTokensAsync revokes every active refresh token for the
//     user, so a stolen token can be invalidated
//   - RefreshAccessTokenAsync rejects revoked / unknown refresh tokens
// </copyright>

using System.IdentityModel.Tokens.Jwt;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Auth;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class JwtServiceTests
{
    private const string TestSecret = "unit-test-secret-key-that-is-at-least-32-chars-long-for-hs256";

    private static (JwtService service, AppDbContext db) NewService()
    {
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseInMemoryDatabase(databaseName: Guid.NewGuid().ToString())
            .Options;
        var db = new AppDbContext(options);
        var settings = new JwtSettings
        {
            Secret = TestSecret,
            Issuer = "test-issuer",
            Audience = "test-audience",
            AccessExpiryMinutes = 15,
            RefreshExpiryDays = 7,
        };
        return (new JwtService(db, settings), db);
    }

    /// <summary>
    /// Build a user and persist it so that any FK lookup
    /// (RefreshToken.UserId -> Users.Id) inside JwtService finds it.
    /// </summary>
    private static User MakeUser(AppDbContext db, string email = "alice@example.com")
    {
        var user = new User
        {
            Id = Guid.NewGuid(),
            Email = email,
            PasswordHash = "not-used-in-these-tests",
            CreatedAt = DateTime.UtcNow,
        };
        db.Users.Add(user);
        db.SaveChanges();
        return user;
    }

    [Fact]
    public async Task GenerateTokensAsync_Emits_Access_And_Refresh()
    {
        var (service, db) = NewService();
        using var _ = db;
        var user = MakeUser(db);

        var tokens = await service.GenerateTokensAsync(user);

        Assert.False(string.IsNullOrEmpty(tokens.AccessToken));
        Assert.False(string.IsNullOrEmpty(tokens.RefreshToken));
        Assert.True(tokens.AccessExpiresAt > DateTime.UtcNow);

        // The access token must carry sub = user id and email claim.
        var handler = new JwtSecurityTokenHandler();
        var jwt = handler.ReadJwtToken(tokens.AccessToken);
        Assert.Equal(user.Id.ToString(), jwt.Subject);
        Assert.Equal(user.Email, jwt.Claims.First(c => c.Type == JwtRegisteredClaimNames.Email).Value);
        Assert.Equal("access", jwt.Claims.First(c => c.Type == "type").Value);
        Assert.Equal("test-audience", jwt.Audiences.Single());
    }

    [Fact]
    public async Task RefreshAccessTokenAsync_Rotates_And_Revokes()
    {
        var (service, db) = NewService();
        using var _ = db;
        var user = MakeUser(db);
        var initial = await service.GenerateTokensAsync(user);

        var refreshed = await service.RefreshAccessTokenAsync(initial.RefreshToken);

        Assert.True(refreshed.Succeeded);
        Assert.False(string.IsNullOrEmpty(refreshed.AccessToken));
        Assert.False(string.IsNullOrEmpty(refreshed.NewRefreshToken));
        Assert.NotEqual(initial.RefreshToken, refreshed.NewRefreshToken);

        // The old refresh token must now be revoked — a second refresh with
        // it must fail.
        var second = await service.RefreshAccessTokenAsync(initial.RefreshToken);
        Assert.False(second.Succeeded);
    }

    [Fact]
    public async Task RefreshAccessTokenAsync_Rejects_Unknown_Token()
    {
        var (service, db) = NewService();
        using var _ = db;
        var result = await service.RefreshAccessTokenAsync("not-a-real-token");
        Assert.False(result.Succeeded);
        Assert.True(string.IsNullOrEmpty(result.AccessToken));
    }

    [Fact]
    public async Task RevokeAllUserTokensAsync_Invalidates_All_Active_Refresh_Tokens()
    {
        var (service, db) = NewService();
        using var _ = db;
        var user = MakeUser(db);
        var t1 = await service.GenerateTokensAsync(user);
        var t2 = await service.GenerateTokensAsync(user);

        await service.RevokeAllUserTokensAsync(user.Id);

        Assert.False((await service.RefreshAccessTokenAsync(t1.RefreshToken)).Succeeded);
        Assert.False((await service.RefreshAccessTokenAsync(t2.RefreshToken)).Succeeded);
    }
}
