using Inkuso.Cloud.Admin.Endpoints;
using Inkuso.Cloud.Api.Endpoints;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class UsageReportingVisibilityTests
{
    private static readonly Guid ModelId =
        Guid.Parse("00000000-0000-0000-0001-000000000001");
    private static readonly DateTime Now =
        new(2026, 8, 24, 12, 0, 0, DateTimeKind.Utc);

    [Fact]
    public async Task Account_Usage_Adds_Search_History_Without_Changing_Chat_Data()
    {
        await using var fixture = await Fixture.CreateAsync();

        var result = await Account.GetUsageAsync(fixture.UserId, fixture.Db);

        Assert.Equal(3, result.Data.Count);
        Assert.Contains(result.Data, item => item.BillingStatus == "settled" && item.CostPoints == 120);
        Assert.Equal(3, result.WebSearchData.Count);
        Assert.Contains(result.WebSearchData, item =>
            item.ProviderId == "baike"
            && item.Query == "settled search"
            && item.CostPoints == 50
            && item.BillingStatus == "settled");
        Assert.Contains(result.WebSearchData, item =>
            item.Query == "released search"
            && item.CostPoints == 0
            && item.BillingStatus == "released");
        Assert.DoesNotContain(result.WebSearchData, item => item.BillingStatus == "started");
    }

    [Fact]
    public async Task Dashboard_Revenue_Includes_Only_Settled_Searches_With_A_Breakdown()
    {
        await using var fixture = await Fixture.CreateAsync();

        var summary = await DashboardEndpoints.GetSummaryAsync(fixture.Db, Now);
        var trend = await DashboardEndpoints.GetUsageTrendAsync(fixture.Db, Now);

        Assert.Equal(160, summary.MonthChatRevenuePoints);
        Assert.Equal(50, summary.MonthWebSearchRevenuePoints);
        Assert.Equal(210, summary.MonthRevenuePoints);
        Assert.Equal(120, summary.TotalWebSearchRevenuePoints);
        Assert.Equal(280, summary.TotalRevenuePoints);
        Assert.Equal(1, summary.MonthWebSearchRequests);
        Assert.Equal(2, summary.TotalWebSearchRequests);

        var today = Assert.Single(trend, point => point.Date == Now.Date);
        Assert.Equal(160, today.ChatCostPoints);
        Assert.Equal(50, today.WebSearchCostPoints);
        Assert.Equal(210, today.CostPoints);
        Assert.Equal(2, today.ChatRequests);
        Assert.Equal(1, today.WebSearchRequests);
    }

    [Fact]
    public async Task Admin_Usage_Defaults_To_Chat_And_All_Separates_Search_Rows()
    {
        await using var fixture = await Fixture.CreateAsync();

        var legacyDefault = await AdminUsageEndpoints.QueryUsageAsync(
            fixture.Db, page: 1, pageSize: 30);
        var all = await AdminUsageEndpoints.QueryUsageAsync(
            fixture.Db, page: 1, pageSize: 30, usageType: "all");

        Assert.NotNull(legacyDefault);
        Assert.Equal(3, legacyDefault.Total);
        Assert.All(legacyDefault.Items, item => Assert.Equal("chat", item.UsageType));

        Assert.NotNull(all);
        Assert.Equal(6, all.Total);
        Assert.Equal(3, all.ChatRecords);
        Assert.Equal(3, all.WebSearchRecords);
        Assert.Contains(all.Items, item =>
            item.UsageType == "search"
            && item.ProviderId == "baike"
            && item.Query == "settled search");
        Assert.DoesNotContain(all.Items, item => item.BillingStatus == "started");
    }

    private sealed class Fixture : IAsyncDisposable
    {
        private readonly SqliteConnection connection;

        private Fixture(SqliteConnection connection, AppDbContext db, Guid userId)
        {
            this.connection = connection;
            Db = db;
            UserId = userId;
        }

        public AppDbContext Db { get; }
        public Guid UserId { get; }

        public static async Task<Fixture> CreateAsync()
        {
            var connection = new SqliteConnection("Data Source=:memory:");
            await connection.OpenAsync();
            var options = new DbContextOptionsBuilder<AppDbContext>()
                .UseSqlite(connection)
                .Options;
            var db = new AppDbContext(options);
            await db.Database.EnsureCreatedAsync();

            var user = new User
            {
                Email = $"report-{Guid.NewGuid():N}@example.test",
                PasswordHash = "test",
                CreatedAt = Now.AddHours(-1),
            };
            db.Users.Add(user);
            db.UsageRecords.AddRange(
                Chat(user.Id, "chat-settled", "settled", 120, 120, Now),
                Chat(user.Id, "chat-debt", "debt", 90, 40, Now),
                Chat(user.Id, "chat-released", "released", 0, 0, Now));
            db.WebSearchUsageRecords.AddRange(
                Search(user.Id, "search-settled", "settled search", "settled", 50, Now),
                Search(user.Id, "search-released", "released search", "released", 0, Now),
                Search(user.Id, "search-started", "started search", "started", 50, Now),
                Search(user.Id, "search-old", "old settled search", "settled", 70, Now.AddDays(-40)));
            await db.SaveChangesAsync();
            return new Fixture(connection, db, user.Id);
        }

        private static UsageRecord Chat(
            Guid userId,
            string requestId,
            string status,
            long costPoints,
            long reservedPoints,
            DateTime recordedAt) => new()
        {
            UserId = userId,
            ModelConfigId = ModelId,
            PromptTokens = 100,
            CompletionTokens = 50,
            CostPoints = costPoints,
            ReservedPoints = reservedPoints,
            RequestId = requestId,
            BillingStatus = status,
            RecordedAt = recordedAt,
        };

        private static WebSearchUsageRecord Search(
            Guid userId,
            string requestId,
            string query,
            string status,
            long costPoints,
            DateTime recordedAt) => new()
        {
            UserId = userId,
            ProviderId = "baike",
            Query = query,
            CostPoints = costPoints,
            ReservedPoints = status == "settled" ? costPoints : 0,
            RequestId = requestId,
            BillingStatus = status,
            RecordedAt = recordedAt,
        };

        public async ValueTask DisposeAsync()
        {
            await Db.DisposeAsync();
            await connection.DisposeAsync();
        }
    }
}
