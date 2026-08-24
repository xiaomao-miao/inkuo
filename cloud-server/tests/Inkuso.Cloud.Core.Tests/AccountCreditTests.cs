using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Entities;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class AccountCreditTests
{
    [Fact]
    public async Task Credit_Pays_Debt_First_And_Resumes_When_Fully_Paid()
    {
        await using var connection = new SqliteConnection("Data Source=:memory:");
        await connection.OpenAsync();
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseSqlite(connection)
            .Options;
        await using var db = new AppDbContext(options);
        await db.Database.EnsureCreatedAsync();
        var user = new User
        {
            Email = $"credit-{Guid.NewGuid():N}@example.test",
            PasswordHash = "test",
            BalancePoints = 20,
            DebtPoints = 100,
            IsSuspended = true,
        };
        db.Users.Add(user);
        await db.SaveChangesAsync();

        var partial = await AccountCredit.ApplyAsync(db, user.Id, 60, default);
        Assert.NotNull(partial);
        Assert.Equal(20, partial.BalancePoints);
        Assert.Equal(40, partial.DebtPoints);
        Assert.True(partial.IsSuspended);

        var complete = await AccountCredit.ApplyAsync(db, user.Id, 50, default);
        Assert.NotNull(complete);
        Assert.Equal(30, complete.BalancePoints);
        Assert.Equal(0, complete.DebtPoints);
        Assert.False(complete.IsSuspended);
    }

    [Fact]
    public async Task Credit_Does_Not_Clear_A_Manual_Debt_Free_Suspension()
    {
        await using var connection = new SqliteConnection("Data Source=:memory:");
        await connection.OpenAsync();
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseSqlite(connection)
            .Options;
        await using var db = new AppDbContext(options);
        await db.Database.EnsureCreatedAsync();
        var user = new User
        {
            Email = $"manual-{Guid.NewGuid():N}@example.test",
            PasswordHash = "test",
            AdminSuspended = true,
            IsSuspended = true,
        };
        db.Users.Add(user);
        await db.SaveChangesAsync();

        var result = await AccountCredit.ApplyAsync(db, user.Id, 100, default);
        Assert.NotNull(result);
        Assert.Equal(100, result.BalancePoints);
        Assert.True(result.IsSuspended);
    }

    [Fact]
    public async Task Credit_Refuses_To_Exceed_Account_Balance_Limit()
    {
        await using var connection = new SqliteConnection("Data Source=:memory:");
        await connection.OpenAsync();
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseSqlite(connection)
            .Options;
        await using var db = new AppDbContext(options);
        await db.Database.EnsureCreatedAsync();
        var user = new User
        {
            Email = $"limit-{Guid.NewGuid():N}@example.test",
            PasswordHash = "test",
            BalancePoints = BillingLimits.MaxAccountBalancePoints - 25,
            DebtPoints = 50,
            IsSuspended = true,
        };
        db.Users.Add(user);
        await db.SaveChangesAsync();

        var result = await AccountCredit.ApplyAsync(db, user.Id, 100, default);

        Assert.Null(result);
        await db.Entry(user).ReloadAsync();
        Assert.Equal(BillingLimits.MaxAccountBalancePoints - 25, user.BalancePoints);
        Assert.Equal(50, user.DebtPoints);
    }

    [Fact]
    public async Task Credit_Can_Reach_Account_Balance_Limit_Exactly()
    {
        await using var connection = new SqliteConnection("Data Source=:memory:");
        await connection.OpenAsync();
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseSqlite(connection)
            .Options;
        await using var db = new AppDbContext(options);
        await db.Database.EnsureCreatedAsync();
        var user = new User
        {
            Email = $"exact-limit-{Guid.NewGuid():N}@example.test",
            PasswordHash = "test",
            BalancePoints = BillingLimits.MaxAccountBalancePoints - 50,
            DebtPoints = 50,
            IsSuspended = true,
        };
        db.Users.Add(user);
        await db.SaveChangesAsync();

        var result = await AccountCredit.ApplyAsync(db, user.Id, 100, default);

        Assert.NotNull(result);
        Assert.Equal(BillingLimits.MaxAccountBalancePoints, result.BalancePoints);
        Assert.Equal(0, result.DebtPoints);
    }

    [Fact]
    public async Task Credit_Rejects_An_Oversized_Single_Grant()
    {
        await using var connection = new SqliteConnection("Data Source=:memory:");
        await connection.OpenAsync();
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseSqlite(connection)
            .Options;
        await using var db = new AppDbContext(options);

        await Assert.ThrowsAsync<ArgumentOutOfRangeException>(() =>
            AccountCredit.ApplyAsync(
                db,
                Guid.NewGuid(),
                BillingLimits.MaxSingleCreditPoints + 1,
                default));
    }
}
