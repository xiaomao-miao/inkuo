using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public sealed class BillingLedgerTests
{
    [Fact]
    public async Task Duplicate_Reservation_Does_Not_Freeze_Twice()
    {
        await using var fixture = await LedgerFixture.CreateAsync();

        var first = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 300, "same-request", default);
        var duplicate = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 300, "same-request", default);

        Assert.Equal(BillingLedger.ReservationState.Reserved, first.State);
        Assert.Equal(BillingLedger.ReservationState.AlreadyPending, duplicate.State);
        await fixture.AssertUserAsync(balance: 1_000, reserved: 300);
        Assert.Equal(1, await fixture.Db.UsageRecords.CountAsync());
    }

    [Fact]
    public async Task Reservation_Rejects_When_Available_Balance_Is_Too_Low()
    {
        await using var fixture = await LedgerFixture.CreateAsync();

        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 700, "first-hold", default);
        var rejected = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 400, "second-hold", default);

        Assert.Equal(BillingLedger.ReservationState.Rejected, rejected.State);
        await fixture.AssertUserAsync(balance: 1_000, reserved: 700);
        Assert.Equal(1, await fixture.Db.UsageRecords.CountAsync());
    }

    [Fact]
    public async Task Release_Does_Not_Mint_Balance_And_Is_Idempotent()
    {
        await using var fixture = await LedgerFixture.CreateAsync();
        var requestId = "release-once";

        var reservation = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 400, requestId, default);
        Assert.Equal(BillingLedger.ReservationState.Reserved, reservation.State);
        await fixture.AssertUserAsync(balance: 1_000, reserved: 400);

        Assert.True(await fixture.Ledger.ReleaseAsync(fixture.UserId, requestId, default));
        await fixture.AssertUserAsync(balance: 1_000, reserved: 0);

        Assert.False(await fixture.Ledger.ReleaseAsync(fixture.UserId, requestId, default));
        await fixture.AssertUserAsync(balance: 1_000, reserved: 0);
        Assert.Equal(1, await fixture.Db.UsageRecords.CountAsync());
        Assert.Equal("released", await fixture.StatusAsync(requestId));
    }

    [Fact]
    public async Task Settlement_Charges_Actual_Cost_Once()
    {
        await using var fixture = await LedgerFixture.CreateAsync();
        var requestId = "settle-once";
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 400, requestId, default);

        var first = await fixture.Ledger.SettleAsync(
            fixture.UserId, fixture.ModelId,
            promptTokens: 250, completionTokens: 0, cachedPromptTokens: 0,
            requestId: requestId, ct: default);

        Assert.True(first.Applied);
        Assert.Equal(250L, first.CostPoints);
        Assert.Equal(250L, first.ChargedPoints);
        Assert.Equal(0L, first.DebtPoints);
        Assert.Equal("settled", first.Status);
        await fixture.AssertUserAsync(balance: 750, reserved: 0);

        var duplicate = await fixture.Ledger.SettleAsync(
            fixture.UserId, fixture.ModelId,
            promptTokens: 250, completionTokens: 0, cachedPromptTokens: 0,
            requestId: requestId, ct: default);
        Assert.False(duplicate.Applied);
        await fixture.AssertUserAsync(balance: 750, reserved: 0);
        Assert.Equal(1, await fixture.Db.UsageRecords.CountAsync());
    }

    [Fact]
    public async Task Settlement_Above_Hold_Charges_Only_Hold_And_Records_Debt()
    {
        await using var fixture = await LedgerFixture.CreateAsync();
        var requestId = "settle-debt";
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 200, requestId, default);

        var result = await fixture.Ledger.SettleAsync(
            fixture.UserId, fixture.ModelId,
            promptTokens: 250, completionTokens: 0, cachedPromptTokens: 0,
            requestId: requestId, ct: default);

        Assert.Equal(250L, result.CostPoints);
        Assert.Equal(200L, result.ChargedPoints);
        Assert.Equal(50L, result.DebtPoints);
        Assert.Equal("debt", result.Status);
        var user = await fixture.UserAsync();
        Assert.Equal(800, user.BalancePoints);
        Assert.Equal(0, user.ReservedPoints);
        Assert.True(user.IsSuspended);
    }

    [Fact]
    public async Task Zero_Cost_Settlement_Releases_Hold_Without_Charging()
    {
        await using var fixture = await LedgerFixture.CreateAsync();
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 400, "zero-cost", default);

        var result = await fixture.Ledger.SettleAsync(
            fixture.UserId, fixture.ModelId,
            promptTokens: 0, completionTokens: 0, cachedPromptTokens: 0,
            requestId: "zero-cost", ct: default);

        Assert.Equal("released", result.Status);
        Assert.Equal(0L, result.CostPoints);
        await fixture.AssertUserAsync(balance: 1_000, reserved: 0);
    }

    [Fact]
    public async Task Multiple_Holds_Remain_Independent()
    {
        await using var fixture = await LedgerFixture.CreateAsync();
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 400, "first", default);
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 500, "second", default);

        await fixture.Ledger.SettleAsync(
            fixture.UserId, fixture.ModelId,
            promptTokens: 300, completionTokens: 0, cachedPromptTokens: 0,
            requestId: "first", ct: default);
        await fixture.AssertUserAsync(balance: 700, reserved: 500);

        await fixture.Ledger.ReleaseAsync(fixture.UserId, "second", default);
        await fixture.AssertUserAsync(balance: 700, reserved: 0);
    }

    [Fact]
    public async Task Stale_Reconciliation_Releases_Only_Expired_Holds()
    {
        await using var fixture = await LedgerFixture.CreateAsync();
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 300, "old", default);
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, fixture.ModelId, 200, "fresh", default);
        await fixture.Db.UsageRecords
            .Where(r => r.RequestId == "old")
            .ExecuteUpdateAsync(s => s.SetProperty(
                r => r.RecordedAt, DateTime.UtcNow.AddMinutes(-30)));

        var released = await fixture.Ledger.ReleaseStaleAsync(
            DateTime.UtcNow.AddMinutes(-15), 100, default);

        Assert.Equal(1, released);
        await fixture.AssertUserAsync(balance: 1_000, reserved: 200);
        Assert.Equal("released", await fixture.StatusAsync("old"));
        Assert.Equal("pending", await fixture.StatusAsync("fresh"));
        Assert.Equal(0, await fixture.Ledger.ReleaseStaleAsync(
            DateTime.UtcNow.AddMinutes(-15), 100, default));
    }

    private sealed class LedgerFixture : IAsyncDisposable
    {
        private readonly SqliteConnection connection;

        private LedgerFixture(
            SqliteConnection connection,
            AppDbContext db,
            BillingLedger ledger,
            Guid userId,
            Guid modelId)
        {
            this.connection = connection;
            Db = db;
            Ledger = ledger;
            UserId = userId;
            ModelId = modelId;
        }

        public AppDbContext Db { get; }
        public BillingLedger Ledger { get; }
        public Guid UserId { get; }
        public Guid ModelId { get; }

        public static async Task<LedgerFixture> CreateAsync()
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
                Email = $"billing-{Guid.NewGuid():N}@example.test",
                PasswordHash = "test",
                BalancePoints = 1_000,
            };
            var model = new ModelConfig
            {
                UpstreamProvider = "test",
                UpstreamBaseUrl = "https://example.test",
                UpstreamApiKey = "test",
                ModelName = "test-model",
                DisplayName = "Test Model",
                InputPricePerMTokens = 1_000m,
                OutputPricePerMTokens = 1_000m,
                CachedInputPricePerMTokens = 1_000m,
            };
            db.AddRange(user, model);
            await db.SaveChangesAsync();
            return new LedgerFixture(
                connection, db, new BillingLedger(db), user.Id, model.Id);
        }

        public Task<User> UserAsync() => Db.Users.AsNoTracking().SingleAsync(u => u.Id == UserId);

        public async Task AssertUserAsync(long balance, long reserved)
        {
            var user = await UserAsync();
            Assert.Equal(balance, user.BalancePoints);
            Assert.Equal(reserved, user.ReservedPoints);
        }

        public Task<string> StatusAsync(string requestId) => Db.UsageRecords.AsNoTracking()
            .Where(r => r.RequestId == requestId)
            .Select(r => r.BillingStatus)
            .SingleAsync();

        public async ValueTask DisposeAsync()
        {
            await Db.DisposeAsync();
            await connection.DisposeAsync();
        }
    }
}
