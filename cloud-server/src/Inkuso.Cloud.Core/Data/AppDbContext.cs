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

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        base.OnModelCreating(modelBuilder);

        // User
        modelBuilder.Entity<User>(e =>
        {
            e.HasIndex(u => u.Email).IsUnique();
            e.Property(u => u.BalanceCents).HasPrecision(12, 4);
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
            e.Property(i => i.FreeQuotaCents).HasPrecision(12, 4);
        });

        // RedemptionCode
        modelBuilder.Entity<RedemptionCode>(e =>
        {
            e.HasIndex(r => r.Code).IsUnique();
            e.Property(r => r.CreditCents).HasPrecision(12, 4);
            e.HasOne(r => r.Plan).WithMany().HasForeignKey(r => r.PlanId);
        });

        // ModelConfig
        modelBuilder.Entity<ModelConfig>(e =>
        {
            e.Property(m => m.InputPricePerMTokens).HasPrecision(12, 6);
            e.Property(m => m.OutputPricePerMTokens).HasPrecision(12, 6);
            e.Property(m => m.CachedInputPricePerMTokens).HasPrecision(12, 6);
        });

        // UsageRecord
        modelBuilder.Entity<UsageRecord>(e =>
        {
            e.Property(u => u.CostCents).HasPrecision(12, 6);
            e.HasOne(u => u.User)
                .WithMany()
                .HasForeignKey(u => u.UserId)
                .OnDelete(DeleteBehavior.Restrict);
            e.HasOne(u => u.ModelConfig)
                .WithMany()
                .HasForeignKey(u => u.ModelConfigId)
                .OnDelete(DeleteBehavior.Restrict);
            e.HasIndex(u => new { u.UserId, u.RecordedAt });
        });

        // Plan
        modelBuilder.Entity<Plan>(e =>
        {
            // MonthlyQuotaCents is an int (whole cents) on the entity, so
            // HasPrecision would be silently ignored by Npgsql. We keep the
            // schema type as `integer`; if a future schema bump ever wants
            // fractional cents, change the entity to decimal first.
            e.Property(p => p.MonthlyQuotaCents);
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

        // Seed default plans
        modelBuilder.Entity<Plan>().HasData(
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000001"), Name = "Free", MonthlyQuotaCents = 0, MonthlyTokenLimit = 500_000, OverageInputPricePer1k = 0.002m, OverageOutputPricePer1k = 0.004m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000002"), Name = "Plus", MonthlyQuotaCents = 2900, MonthlyTokenLimit = 5_000_000, OverageInputPricePer1k = 0.002m, OverageOutputPricePer1k = 0.004m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000003"), Name = "Pro", MonthlyQuotaCents = 9900, MonthlyTokenLimit = 25_000_000, OverageInputPricePer1k = 0.0015m, OverageOutputPricePer1k = 0.003m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new Plan { Id = Guid.Parse("00000000-0000-0000-0000-000000000004"), Name = "Max", MonthlyQuotaCents = 29900, MonthlyTokenLimit = 100_000_000, OverageInputPricePer1k = 0.001m, OverageOutputPricePer1k = 0.002m, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
        );

        // Seed default invite code (INKUO2026 for early adopters)
        modelBuilder.Entity<InviteCode>().HasData(
            new InviteCode { Id = 1, Code = "INKUO2026", FreeQuotaCents = 500, MaxUses = 9999, Enabled = true, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
        );

        // Seed default model configs
        modelBuilder.Entity<ModelConfig>().HasData(
            new ModelConfig { Id = Guid.Parse("00000000-0000-0000-0001-000000000001"), UpstreamProvider = "deepseek", UpstreamBaseUrl = "https://api.deepseek.com", ModelName = "deepseek-chat", DisplayName = "DeepSeek-V3", InputPricePerMTokens = 1.0m, OutputPricePerMTokens = 2.0m, CachedInputPricePerMTokens = 0.1m, Enabled = true, SortOrder = 1, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new ModelConfig { Id = Guid.Parse("00000000-0000-0000-0001-000000000002"), UpstreamProvider = "openai", UpstreamBaseUrl = "https://api.openai.com/v1", ModelName = "gpt-4o-mini", DisplayName = "GPT-4o Mini", InputPricePerMTokens = 0.15m, OutputPricePerMTokens = 0.6m, CachedInputPricePerMTokens = 0.075m, Enabled = true, SortOrder = 2, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) },
            new ModelConfig { Id = Guid.Parse("00000000-0000-0000-0001-000000000003"), UpstreamProvider = "openai", UpstreamBaseUrl = "https://api.openai.com/v1", ModelName = "gpt-4o", DisplayName = "GPT-4o", InputPricePerMTokens = 2.5m, OutputPricePerMTokens = 10.0m, CachedInputPricePerMTokens = 1.25m, Enabled = true, SortOrder = 3, CreatedAt = new DateTime(2025, 1, 1, 0, 0, 0, DateTimeKind.Utc) }
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