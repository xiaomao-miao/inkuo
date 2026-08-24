using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Admin.Endpoints;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

/// <summary>
/// Exercises the billing state machine against PostgreSQL's real transaction,
/// row-lock and unique-index semantics. The regular local test run remains
/// self-contained: when no integration connection string is supplied these
/// tests return without touching a database.
/// </summary>
public sealed class PostgresBillingConcurrencyTests
{
    private const string ConnectionVariable = "INKUSO_TEST_POSTGRES";

    [Fact]
    public async Task Account_Credit_Enforces_The_Cap_With_PostgreSql_ExecuteUpdate()
    {
        var connectionString = Environment.GetEnvironmentVariable(ConnectionVariable);
        if (string.IsNullOrWhiteSpace(connectionString)) return;

        await using var fixture = await PostgresLedgerFixture.CreateAsync(connectionString);
        await using (var setup = fixture.CreateContext())
        {
            await setup.Users
                .Where(user => user.Id == fixture.UserId)
                .ExecuteUpdateAsync(update => update
                    .SetProperty(
                        user => user.BalancePoints,
                        BillingLimits.MaxAccountBalancePoints - 25)
                    .SetProperty(user => user.DebtPoints, 50L)
                    .SetProperty(user => user.IsSuspended, true));
        }

        await using (var overLimitDb = fixture.CreateContext())
        {
            var rejected = await AccountCredit.ApplyAsync(
                overLimitDb,
                fixture.UserId,
                creditPoints: 100,
                CancellationToken.None);
            Assert.Null(rejected);
        }

        await using (var exactLimitDb = fixture.CreateContext())
        {
            var accepted = await AccountCredit.ApplyAsync(
                exactLimitDb,
                fixture.UserId,
                creditPoints: 75,
                CancellationToken.None);
            Assert.NotNull(accepted);
            Assert.Equal(BillingLimits.MaxAccountBalancePoints, accepted.BalancePoints);
            Assert.Equal(0, accepted.DebtPoints);
            Assert.False(accepted.IsSuspended);
        }
    }

    [Fact]
    public async Task Combined_Admin_Usage_Paginates_Both_PostgreSql_Ledgers()
    {
        var connectionString = Environment.GetEnvironmentVariable(ConnectionVariable);
        if (string.IsNullOrWhiteSpace(connectionString)) return;

        await using var fixture = await PostgresLedgerFixture.CreateAsync(connectionString);
        await using (var seed = fixture.CreateContext())
        {
            var recordedAt = DateTime.UtcNow;
            seed.UsageRecords.Add(new UsageRecord
            {
                UserId = fixture.UserId,
                ModelConfigId = fixture.ModelId,
                PromptTokens = 12,
                CompletionTokens = 3,
                CostPoints = 9,
                BillingStatus = "settled",
                RequestId = $"usage-report-{Guid.NewGuid():N}",
                RecordedAt = recordedAt,
            });
            seed.WebSearchUsageRecords.Add(new WebSearchUsageRecord
            {
                UserId = fixture.UserId,
                ProviderId = "baike",
                Query = "postgres reporting",
                CostPoints = 50,
                ReservedPoints = 50,
                BillingStatus = "settled",
                RequestId = $"search-report-{Guid.NewGuid():N}",
                RecordedAt = recordedAt.AddSeconds(1),
            });
            await seed.SaveChangesAsync();
        }

        await using var queryDb = fixture.CreateContext();
        var response = await AdminUsageEndpoints.QueryUsageAsync(
            queryDb,
            page: 1,
            pageSize: 20,
            userId: fixture.UserId,
            usageType: "all",
            ct: CancellationToken.None);

        Assert.NotNull(response);
        Assert.Equal(2, response.Total);
        Assert.Equal(59, response.TotalCostPoints);
        Assert.Collection(
            response.Items,
            item => Assert.Equal("search", item.UsageType),
            item => Assert.Equal("chat", item.UsageType));
    }

    [Fact]
    public async Task Concurrent_Reservations_For_One_Request_Freeze_Only_Once()
    {
        var connectionString = Environment.GetEnvironmentVariable(ConnectionVariable);
        if (string.IsNullOrWhiteSpace(connectionString)) return;

        await using var fixture = await PostgresLedgerFixture.CreateAsync(connectionString);
        const int contenderCount = 8;
        const long hold = 400;
        var requestId = $"reserve-{Guid.NewGuid():N}";
        var start = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        var allReady = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        var readyCount = 0;

        var attempts = Enumerable.Range(0, contenderCount).Select(async _ =>
        {
            await using var db = fixture.CreateContext();
            var ledger = new BillingLedger(db);
            if (Interlocked.Increment(ref readyCount) == contenderCount)
                allReady.TrySetResult(true);
            await start.Task;
            return await ledger.TryReserveAsync(
                fixture.UserId,
                fixture.ModelId,
                hold,
                requestId,
                CancellationToken.None);
        }).ToArray();

        await allReady.Task;
        start.TrySetResult(true);
        var results = await Task.WhenAll(attempts);

        Assert.Single(results, result =>
            result.State == BillingLedger.ReservationState.Reserved);
        Assert.Equal(contenderCount - 1, results.Count(result =>
            result.State == BillingLedger.ReservationState.AlreadyPending));

        await using var verification = fixture.CreateContext();
        var user = await verification.Users.AsNoTracking()
            .SingleAsync(candidate => candidate.Id == fixture.UserId);
        var usages = await verification.UsageRecords.AsNoTracking()
            .Where(usage => usage.UserId == fixture.UserId && usage.RequestId == requestId)
            .ToListAsync();

        Assert.Equal(1_000, user.BalancePoints);
        Assert.Equal(hold, user.ReservedPoints);
        Assert.InRange(user.ReservedPoints, 0L, user.BalancePoints);
        var usage = Assert.Single(usages);
        Assert.Equal(hold, usage.ReservedPoints);
        Assert.Equal("pending", usage.BillingStatus);
    }

    [Fact]
    public async Task Concurrent_Settle_And_Release_Produce_One_Terminal_State()
    {
        var connectionString = Environment.GetEnvironmentVariable(ConnectionVariable);
        if (string.IsNullOrWhiteSpace(connectionString)) return;

        await using var fixture = await PostgresLedgerFixture.CreateAsync(connectionString);
        const long hold = 400;
        const long expectedCharge = 200;
        var requestId = $"terminal-{Guid.NewGuid():N}";

        await using (var reservationDb = fixture.CreateContext())
        {
            var reservation = await new BillingLedger(reservationDb).TryReserveAsync(
                fixture.UserId,
                fixture.ModelId,
                hold,
                requestId,
                CancellationToken.None);
            Assert.Equal(BillingLedger.ReservationState.Reserved, reservation.State);
        }

        var start = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        var bothReady = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        var readyCount = 0;

        async Task<BillingLedger.SettlementResult> SettleAsync()
        {
            await using var db = fixture.CreateContext();
            var ledger = new BillingLedger(db);
            if (Interlocked.Increment(ref readyCount) == 2) bothReady.TrySetResult(true);
            await start.Task;
            return await ledger.SettleAsync(
                fixture.UserId,
                fixture.ModelId,
                promptTokens: 200,
                completionTokens: 0,
                cachedPromptTokens: 0,
                requestId: requestId,
                ct: CancellationToken.None);
        }

        async Task<bool> ReleaseAsync()
        {
            await using var db = fixture.CreateContext();
            var ledger = new BillingLedger(db);
            if (Interlocked.Increment(ref readyCount) == 2) bothReady.TrySetResult(true);
            await start.Task;
            return await ledger.ReleaseAsync(
                fixture.UserId,
                requestId,
                CancellationToken.None);
        }

        var settlementTask = SettleAsync();
        var releaseTask = ReleaseAsync();
        await bothReady.Task;
        start.TrySetResult(true);
        await Task.WhenAll(settlementTask, releaseTask);

        var settlement = await settlementTask;
        var released = await releaseTask;
        Assert.Equal(1, (settlement.Applied ? 1 : 0) + (released ? 1 : 0));

        await using var verification = fixture.CreateContext();
        var user = await verification.Users.AsNoTracking()
            .SingleAsync(candidate => candidate.Id == fixture.UserId);
        var usage = await verification.UsageRecords.AsNoTracking()
            .SingleAsync(candidate =>
                candidate.UserId == fixture.UserId && candidate.RequestId == requestId);

        Assert.Equal(0, user.ReservedPoints);
        Assert.InRange(user.ReservedPoints, 0L, user.BalancePoints);
        Assert.Equal(0L, user.DebtPoints);
        Assert.False(user.IsSuspended);

        if (settlement.Applied)
        {
            Assert.False(released);
            Assert.Equal("settled", usage.BillingStatus);
            Assert.Equal(expectedCharge, usage.CostPoints);
            Assert.Equal(1_000 - expectedCharge, user.BalancePoints);
        }
        else
        {
            Assert.True(released);
            Assert.Equal("released", usage.BillingStatus);
            Assert.Equal(0, usage.CostPoints);
            Assert.Equal(1_000, user.BalancePoints);
        }
    }

    private sealed class PostgresLedgerFixture : IAsyncDisposable
    {
        private readonly DbContextOptions<AppDbContext> options;

        private PostgresLedgerFixture(
            DbContextOptions<AppDbContext> options,
            Guid userId,
            Guid modelId)
        {
            this.options = options;
            UserId = userId;
            ModelId = modelId;
        }

        public Guid UserId { get; }
        public Guid ModelId { get; }

        public static async Task<PostgresLedgerFixture> CreateAsync(string connectionString)
        {
            var options = new DbContextOptionsBuilder<AppDbContext>()
                .UseNpgsql(connectionString)
                .Options;
            var user = new User
            {
                Email = $"postgres-billing-{Guid.NewGuid():N}@example.test",
                PasswordHash = "integration-test",
                BalancePoints = 1_000,
            };
            var model = new ModelConfig
            {
                UpstreamProvider = "integration-test",
                UpstreamBaseUrl = "https://example.test",
                UpstreamApiKey = "integration-test",
                ModelName = $"postgres-{Guid.NewGuid():N}",
                DisplayName = "PostgreSQL billing integration test",
                InputPricePerMTokens = 1_000m,
                OutputPricePerMTokens = 1_000m,
                CachedInputPricePerMTokens = 1_000m,
            };

            await using var db = new AppDbContext(options);
            db.AddRange(user, model);
            await db.SaveChangesAsync();
            return new PostgresLedgerFixture(options, user.Id, model.Id);
        }

        public AppDbContext CreateContext() => new(options);

        public async ValueTask DisposeAsync()
        {
            await using var db = CreateContext();
            await db.WebSearchUsageRecords
                .Where(usage => usage.UserId == UserId)
                .ExecuteDeleteAsync();
            await db.UsageRecords
                .Where(usage => usage.UserId == UserId)
                .ExecuteDeleteAsync();
            await db.Users
                .Where(user => user.Id == UserId)
                .ExecuteDeleteAsync();
            await db.ModelConfigs
                .Where(model => model.Id == ModelId)
                .ExecuteDeleteAsync();
        }
    }
}
