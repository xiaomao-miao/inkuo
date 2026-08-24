using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Entities;

namespace Inkuso.Cloud.Core.Data;

public class AppDbContext : DbContext
{
    public AppDbContext(DbContextOptions<AppDbContext> options) : base(options) { }

    public DbSet<User> Users => Set<User>();
    public DbSet<RefreshToken> RefreshTokens => Set<RefreshToken>();
    public DbSet<Plan> Plans => Set<Plan>();
    public DbSet<Subscription> Subscriptions => Set<Subscription>();
    public DbSet<InviteCode> InviteCodes => Set<InviteCode>();
    public DbSet<RedemptionCode> RedemptionCodes => Set<RedemptionCode>();
    public DbSet<ModelConfig> ModelConfigs => Set<ModelConfig>();
    public DbSet<UsageRecord> UsageRecords => Set<UsageRecord>();
    public DbSet<AdminUser> AdminUsers => Set<AdminUser>();
    public DbSet<WebSearchProvider> WebSearchProviders => Set<WebSearchProvider>();
    public DbSet<WebSearchUsageRecord> WebSearchUsageRecords => Set<WebSearchUsageRecord>();
    public DbSet<Release> Releases => Set<Release>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        // User
        modelBuilder.Entity<User>(e =>
        {
            e.HasIndex(u => u.Email).IsUnique();
            // Account currency is points (1 元 = 1000 点). Whole points only — no fractional.
            e.Property(u => u.BalancePoints);
            e.Property(u => u.ReservedPoints);
        });

        // RefreshToken
        modelBuilder.Entity<RefreshToken>(e =>
        {
            e.HasIndex(rt => rt.Jti).IsUnique();
            e.HasOne(rt => rt.User)
                .WithMany()
                .HasForeignKey(rt => rt.UserId)
                .OnDelete(DeleteBehavior.Restrict);
        });

        // Subscription
        modelBuilder.Entity<Subscription>(e =>
        {
            e.HasOne(s => s.User)
                .WithMany()
                .HasForeignKey(s => s.UserId)
                .OnDelete(DeleteBehavior.Restrict);
            e.HasOne(s => s.Plan)
                .WithMany()
                .HasForeignKey(s => s.PlanId)
                .OnDelete(DeleteBehavior.Restrict);
            e.Property(s => s.Status).HasMaxLength(32);
        });

        // InviteCode
        modelBuilder.Entity<InviteCode>(e =>
        {
            e.HasIndex(i => i.Code).IsUnique();
        });

        // RedemptionCode
        modelBuilder.Entity<RedemptionCode>(e =>
        {
            e.HasIndex(r => r.Code).IsUnique();
            e.HasOne(r => r.Plan).WithMany().HasForeignKey(r => r.PlanId);
        });

        // ModelConfig
        modelBuilder.Entity<ModelConfig>(e =>
        {
            // Prices are stored as yuan per 1M tokens and converted to points internally
            // (1 元 = 1000 点) at billing time. Keeping the storage unit yuan keeps the
            // admin UI intuitive while the wire-level accounting is in whole points.
            e.Property(m => m.InputPricePerMTokens).HasPrecision(12, 6);
            e.Property(m => m.OutputPricePerMTokens).HasPrecision(12, 6);
            e.Property(m => m.CachedInputPricePerMTokens).HasPrecision(12, 6);
        });

        // UsageRecord
        modelBuilder.Entity<UsageRecord>(e =>
        {
            e.HasOne(u => u.User)
                .WithMany()
                .HasForeignKey(u => u.UserId)
                .OnDelete(DeleteBehavior.Restrict);
            e.HasOne(u => u.ModelConfig)
                .WithMany()
                .HasForeignKey(u => u.ModelConfigId)
                .OnDelete(DeleteBehavior.Restrict);
            e.HasIndex(u => new { u.UserId, u.RecordedAt });
            // A request is a single immutable billing lifecycle. This unique
            // key is the database backstop for retry/idempotency races.
            e.HasIndex(u => new { u.UserId, u.RequestId }).IsUnique();
            e.Property(u => u.RequestId).HasMaxLength(64);
            e.Property(u => u.BillingStatus).HasMaxLength(16);
        });

        // Plan
        modelBuilder.Entity<Plan>(e =>
        {
            // MonthlyPricePoints is a whole-points integer; OverageXPricePer1k fields
            // are yuan-per-1k tokens (admin-friendly unit) and converted to points
            // on the fly during billing.
            e.Property(p => p.MonthlyPricePoints);
            e.Property(p => p.OverageInputPricePer1k).HasPrecision(12, 6);
            e.Property(p => p.OverageOutputPricePer1k).HasPrecision(12, 6);
        });

        // AdminUser
        modelBuilder.Entity<AdminUser>(e =>
        {
            e.HasIndex(u => u.Username).IsUnique();
            e.Property(u => u.Role).HasMaxLength(32);
        });

        // WebSearchProvider
        modelBuilder.Entity<WebSearchProvider>(e =>
        {
            e.HasIndex(p => p.ProviderId).IsUnique();
            e.Property(p => p.ProviderId).HasMaxLength(64);
            e.Property(p => p.DisplayName).HasMaxLength(128);
            e.Property(p => p.UpstreamBaseUrl).HasMaxLength(512);
        });

        // WebSearchUsageRecord
        modelBuilder.Entity<WebSearchUsageRecord>(e =>
        {
            e.HasOne(u => u.User)
                .WithMany()
                .HasForeignKey(u => u.UserId)
                .OnDelete(DeleteBehavior.Restrict);
            e.Property(u => u.ProviderId).HasMaxLength(64);
            e.Property(u => u.Query).HasMaxLength(512);
            e.HasIndex(u => new { u.UserId, u.RecordedAt });
        });

        // Release
        modelBuilder.Entity<Release>(e =>
        {
            e.Property(r => r.Version).HasMaxLength(64);
            e.Property(r => r.Channel).HasMaxLength(32);
            e.Property(r => r.Platform).HasMaxLength(32);
            e.Property(r => r.Architecture).HasMaxLength(32);
            e.Property(r => r.FileName).HasMaxLength(256);
            e.Property(r => r.Sha256).HasMaxLength(128);
            e.Property(r => r.StoragePath).HasMaxLength(512);
            e.Property(r => r.DownloadUrl).HasMaxLength(512);
            // Prevent publishing the same artifact twice.
            e.HasIndex(r => new { r.Platform, r.Architecture, r.Channel, r.Version }).IsUnique();
            e.HasIndex(r => new { r.Enabled, r.IsLatest });
            e.HasIndex(r => r.CreatedAt);
        });

        // Seed default plans (prices in points, 1 元 = 1000 点)
        modelBuilder.Entity<Plan>().HasData(
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000001"), Name = "Free", MonthlyPricePoints = 0, MonthlyTokenLimit = 500_000, OverageInputPricePer1k = 0.002m, OverageOutputPricePer1k = 0.004m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000002"), Name = "Plus", MonthlyPricePoints = 29_000, MonthlyTokenLimit = 5_000_000, OverageInputPricePer1k = 0.002m, OverageOutputPricePer1k = 0.004m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000003"), Name = "Pro", MonthlyPricePoints = 99_000, MonthlyTokenLimit = 25_000_000, OverageInputPricePer1k = 0.0015m, OverageOutputPricePer1k = 0.003m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000004"), Name = "Max", MonthlyPricePoints = 299_000, MonthlyTokenLimit = 100_000_000, OverageInputPricePer1k = 0.001m, OverageOutputPricePer1k = 0.002m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
        );

        // Seed default invite code (INKUO2026 for early adopters). 5000 points = ¥5 new-user credit.
        modelBuilder.Entity<InviteCode>().HasData(
            new InviteCode { Id = 1, Code = "INKUO2026", FreePoints = 5000, MaxUses = 9999, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
        );

        // Seed default model configs (prices in yuan per 1M tokens, converted to points at billing time)
        modelBuilder.Entity<ModelConfig>().HasData(
            new ModelConfig { Id = Guid.Parse("00000000-0000-0000-0001-000000000001"), UpstreamProvider = "deepseek", UpstreamBaseUrl = "https://api.deepseek.com", ModelName = "deepseek-chat", DisplayName = "DeepSeek-V3", InputPricePerMTokens = 1.0m, OutputPricePerMTokens = 2.0m, CachedInputPricePerMTokens = 0.1m, Enabled = true, SortOrder = 1, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new ModelConfig { Id = Guid.Parse("00000000-0000-0000-0001-000000000002"), UpstreamProvider = "openai", UpstreamBaseUrl = "https://api.openai.com/v1", ModelName = "gpt-4o-mini", DisplayName = "GPT-4o Mini", InputPricePerMTokens = 0.15m, OutputPricePerMTokens = 0.6m, CachedInputPricePerMTokens = 0.075m, Enabled = true, SortOrder = 2, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new ModelConfig { Id = Guid.Parse("00000000-0000-0000-0001-000000000003"), UpstreamProvider = "openai", UpstreamBaseUrl = "https://api.openai.com/v1", ModelName = "gpt-4o", DisplayName = "GPT-4o", InputPricePerMTokens = 2.5m, OutputPricePerMTokens = 10.0m, CachedInputPricePerMTokens = 1.25m, Enabled = true, SortOrder = 3, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
        );

        // Seed one starter redemption code (5000 points = ¥5) so the admin UI has
        // an example row immediately after a fresh deploy. Admins can disable or
        // delete it; the seed is just a convenience.
        modelBuilder.Entity<RedemptionCode>().HasData(
            new RedemptionCode { Id = 1, Code = "WELCOME-5000", CreditPoints = 5_000, MaxUses = 9999, UsedCount = 0, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
        );

        // Seed default web_search provider. The Baidu Baike endpoint
        // requires an operator-supplied API key in production; the seed
        // has no key so the desktop client sees a clear "missing key"
        // error until the operator pastes one in the admin UI.
        modelBuilder.Entity<WebSearchProvider>().HasData(
            new WebSearchProvider
            {
                Id = Guid.Parse("00000000-0000-0000-0002-000000000001"),
                ProviderId = "baike",
                DisplayName = "百度百科",
                UpstreamBaseUrl = "https://appbuilder.baidu.com/v2/baike/lemma/get_content",
                UpstreamApiKey = null,
                Enabled = true,
                CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc),
            }
        );

        // Seed default admin user (NOT via HasData — see §2 below).
        //
        // §1 Why not HasData?
        //    BCrypt hashes are non-deterministic (each call uses a random salt),
        //    so every `dotnet ef migrations add` run would generate a different
        //    migration snapshot, polluting git history with unrelated changes.
        //    Instead the admin is provisioned at first startup by AdminService
        //    (wired in Inkuso.Cloud.Admin/Program.cs via EnsureSeedAdminAsync).
        //
        // §2 If you need a deterministic seed admin for local dev / test
        //    environments, set Admin__SeedUsername + Admin__SeedPassword in
        //    appsettings.Development.json. The password is hashed once on startup,
        //    not baked into the migration.
    }
}
