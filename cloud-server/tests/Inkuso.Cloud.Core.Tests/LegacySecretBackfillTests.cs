using Microsoft.AspNetCore.DataProtection;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Security;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class LegacySecretBackfillTests
{
    [Fact]
    public async Task ProtectAsync_Encrypts_Legacy_Rows_And_Is_Idempotent()
    {
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseInMemoryDatabase(Guid.NewGuid().ToString())
            .Options;
        await using var db = new AppDbContext(options);

        var dataProtection = DataProtectionProvider.Create("inkuo-cloud-backfill-test");
        ISecretProtector protector = new DataProtectionSecretProtector(
            dataProtection.CreateProtector(DataProtectionSecretProtector.Purpose));
        var alreadyProtected = protector.Protect("sk-already-protected")!;

        var legacyModel = new ModelConfig
        {
            ModelName = "legacy-model",
            DisplayName = "Legacy model",
            UpstreamBaseUrl = "https://example.invalid",
            UpstreamApiKey = "sk-legacy-model",
        };
        var protectedModel = new ModelConfig
        {
            ModelName = "protected-model",
            DisplayName = "Protected model",
            UpstreamBaseUrl = "https://example.invalid",
            UpstreamApiKey = alreadyProtected,
        };
        var legacySearch = new WebSearchProvider
        {
            ProviderId = "legacy-search",
            DisplayName = "Legacy search",
            UpstreamApiKey = "search-legacy-key",
        };
        db.AddRange(legacyModel, protectedModel, legacySearch);
        await db.SaveChangesAsync();

        Assert.Equal(2, await LegacySecretBackfill.ProtectAsync(db, protector));
        Assert.True(protector.IsProtected(legacyModel.UpstreamApiKey));
        Assert.True(protector.IsProtected(legacySearch.UpstreamApiKey));
        Assert.Equal("sk-legacy-model", protector.Unprotect(legacyModel.UpstreamApiKey));
        Assert.Equal("search-legacy-key", protector.Unprotect(legacySearch.UpstreamApiKey));
        Assert.Equal(alreadyProtected, protectedModel.UpstreamApiKey);

        Assert.Equal(0, await LegacySecretBackfill.ProtectAsync(db, protector));
    }
}
