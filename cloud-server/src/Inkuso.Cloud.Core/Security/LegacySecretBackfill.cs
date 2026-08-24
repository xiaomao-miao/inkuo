using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Core.Security;

/// <summary>
/// Idempotently upgrades provider credentials written before at-rest
/// protection was introduced. This runs after migrations and before the
/// Admin API starts serving requests, so plaintext credentials never remain
/// indefinitely just because an operator has not edited the row again.
/// </summary>
public static class LegacySecretBackfill
{
    public static async Task<int> ProtectAsync(
        AppDbContext db,
        ISecretProtector protector,
        CancellationToken cancellationToken = default)
    {
        var changed = 0;
        var modelConfigs = await db.ModelConfigs
            .Where(model => model.UpstreamApiKey != string.Empty)
            .ToListAsync(cancellationToken);

        foreach (var model in modelConfigs)
        {
            if (protector.IsProtected(model.UpstreamApiKey)) continue;
            model.UpstreamApiKey = protector.Protect(model.UpstreamApiKey) ?? string.Empty;
            changed++;
        }

        var webSearchProviders = await db.WebSearchProviders
            .Where(provider => provider.UpstreamApiKey != null && provider.UpstreamApiKey != string.Empty)
            .ToListAsync(cancellationToken);

        foreach (var provider in webSearchProviders)
        {
            if (protector.IsProtected(provider.UpstreamApiKey)) continue;
            provider.UpstreamApiKey = protector.Protect(provider.UpstreamApiKey);
            changed++;
        }

        if (changed > 0)
            await db.SaveChangesAsync(cancellationToken);

        return changed;
    }
}
