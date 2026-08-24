using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class WebSearchLedgerTests
{
    [Fact]
    public async Task Duplicate_Request_Reserves_And_Charges_Only_Once()
    {
        await using var fixture = await Fixture.CreateAsync();
        var first = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "inkuo", 50, "same-search", default);
        var duplicate = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "inkuo", 50, "same-search", default);

        Assert.True(first.CanForward);
        Assert.Equal(WebSearchLedger.ReservationState.Duplicate, duplicate.State);
        await fixture.AssertUserAsync(balance: 1_000, reserved: 50);
        Assert.True(await fixture.Ledger.MarkStartedAsync(
            fixture.UserId, "same-search", default));
        Assert.True(await fixture.Ledger.SettleAsync(
            fixture.UserId, "same-search", default));
        Assert.False(await fixture.Ledger.SettleAsync(
            fixture.UserId, "same-search", default));
        await fixture.AssertUserAsync(balance: 950, reserved: 0);
    }

    [Fact]
    public async Task Known_Failure_Releases_Pending_Or_Started_Hold()
    {
        await using var fixture = await Fixture.CreateAsync();
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "first", 50, "pending-failure", default);
        Assert.True(await fixture.Ledger.ReleaseAsync(
            fixture.UserId, "pending-failure", default));

        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "second", 50, "started-failure", default);
        await fixture.Ledger.MarkStartedAsync(
            fixture.UserId, "started-failure", default);
        Assert.True(await fixture.Ledger.ReleaseAsync(
            fixture.UserId, "started-failure", default));

        await fixture.AssertUserAsync(balance: 1_000, reserved: 0);
        Assert.Equal(2, await fixture.Db.WebSearchUsageRecords.CountAsync(
            record => record.BillingStatus == "released"));
    }

    [Fact]
    public async Task Reconciliation_Charges_Started_And_Releases_Unstarted()
    {
        await using var fixture = await Fixture.CreateAsync();
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "accepted", 50, "accepted", default);
        await fixture.Ledger.MarkStartedAsync(fixture.UserId, "accepted", default);
        await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "not-started", 50, "not-started", default);
        await fixture.Db.WebSearchUsageRecords.ExecuteUpdateAsync(update => update.SetProperty(
            record => record.RecordedAt,
            DateTime.UtcNow.AddMinutes(-5)));

        var cutoff = DateTime.UtcNow.AddMinutes(-2);
        Assert.Equal(1, await fixture.Ledger.SettleStaleStartedAsync(cutoff, 100, default));
        Assert.Equal(1, await fixture.Ledger.ReleaseStalePendingAsync(cutoff, 100, default));
        await fixture.AssertUserAsync(balance: 950, reserved: 0);
        Assert.Equal("settled", await fixture.StatusAsync("accepted"));
        Assert.Equal("released", await fixture.StatusAsync("not-started"));
    }

    [Fact]
    public async Task Rejection_Distinguishes_Admin_Suspension_From_Low_Balance()
    {
        await using var fixture = await Fixture.CreateAsync();
        await fixture.Db.Users.Where(user => user.Id == fixture.UserId)
            .ExecuteUpdateAsync(update => update
                .SetProperty(user => user.AdminSuspended, true)
                .SetProperty(user => user.IsSuspended, true));

        var suspended = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "blocked", 50, "blocked", default);
        Assert.Equal(WebSearchLedger.ReservationState.Rejected, suspended.State);
        Assert.Equal("admin_suspended", suspended.RejectionReason);

        await fixture.Db.Users.Where(user => user.Id == fixture.UserId)
            .ExecuteUpdateAsync(update => update
                .SetProperty(user => user.AdminSuspended, false)
                .SetProperty(user => user.IsSuspended, false)
                .SetProperty(user => user.BalancePoints, 0L));

        var empty = await fixture.Ledger.TryReserveAsync(
            fixture.UserId, "baike", "empty", 50, "empty", default);
        Assert.Equal(WebSearchLedger.ReservationState.Rejected, empty.State);
        Assert.Equal("insufficient_points", empty.RejectionReason);
    }

    private sealed class Fixture : IAsyncDisposable
    {
        private readonly SqliteConnection connection;

        private Fixture(SqliteConnection connection, AppDbContext db, Guid userId)
        {
            this.connection = connection;
            Db = db;
            UserId = userId;
            Ledger = new WebSearchLedger(db);
        }

        public AppDbContext Db { get; }
        public Guid UserId { get; }
        public WebSearchLedger Ledger { get; }

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
                Email = $"search-{Guid.NewGuid():N}@example.test",
                PasswordHash = "test",
                BalancePoints = 1_000,
            };
            db.Users.Add(user);
            await db.SaveChangesAsync();
            return new Fixture(connection, db, user.Id);
        }

        public async Task AssertUserAsync(long balance, long reserved)
        {
            var user = await Db.Users.AsNoTracking().SingleAsync(user => user.Id == UserId);
            Assert.Equal(balance, user.BalancePoints);
            Assert.Equal(reserved, user.ReservedPoints);
        }

        public Task<string> StatusAsync(string requestId) => Db.WebSearchUsageRecords
            .AsNoTracking()
            .Where(record => record.RequestId == requestId)
            .Select(record => record.BillingStatus)
            .SingleAsync();

        public async ValueTask DisposeAsync()
        {
            await Db.DisposeAsync();
            await connection.DisposeAsync();
        }
    }
}
