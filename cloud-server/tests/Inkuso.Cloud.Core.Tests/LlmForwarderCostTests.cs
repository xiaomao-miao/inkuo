// <copyright file="LlmForwarderCostTests.cs" company="inkuo">
// Unit tests for LlmForwarder.CalculateCostPoints — the cost math is the
// single source of truth for what users are billed, so we want regression
// coverage on edge cases (zero tokens, cached >= prompt, rounding to whole
// points).
// </copyright>

using Inkuso.Cloud.Core.Entities;
using Inkuso.Cloud.Core.Upstream;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class LlmForwarderCostTests
{
    private static ModelConfig MakeConfig(
        decimal inputPrice = 10m,
        decimal outputPrice = 20m,
        decimal cachedPrice = 1m) => new()
    {
        InputPricePerMTokens = inputPrice,
        OutputPricePerMTokens = outputPrice,
        CachedInputPricePerMTokens = cachedPrice,
    };

    [Fact]
    public void Zero_Usage_Yields_Zero_Cost()
    {
        var config = MakeConfig();
        Assert.Equal(0L, LlmForwarder.CalculateCostPoints(config, 0, 0, 0));
    }

    [Fact]
    public void Input_Only_Bills_At_Input_Price()
    {
        var config = MakeConfig(inputPrice: 10m);
        // 1M tokens * 10 yuan/1M * 1000 (yuan→points) = 10_000 points
        Assert.Equal(10_000L, LlmForwarder.CalculateCostPoints(config, 1_000_000, 0));
    }

    [Fact]
    public void Output_Only_Bills_At_Output_Price()
    {
        var config = MakeConfig(outputPrice: 25m);
        // 1M tokens * 25 yuan/1M * 1000 = 25_000 points
        Assert.Equal(25_000L, LlmForwarder.CalculateCostPoints(config, 0, 1_000_000));
    }

    [Fact]
    public void Cached_Tokens_Subtract_From_Prompt_Total()
    {
        var config = MakeConfig(inputPrice: 10m, cachedPrice: 1m);
        // 1M prompt total, 600k cached, 400k uncached
        // cached: 600k/1M * 1   = 0.6 yuan  ->  600 points
        // input:  400k/1M * 10  = 4.0 yuan  -> 4000 points
        // output: 0
        // total:  4.6 yuan = 4600 points
        var cost = LlmForwarder.CalculateCostPoints(config, 1_000_000, 0, 600_000);
        Assert.Equal(4600L, cost);
    }

    [Fact]
    public void Cached_Tokens_Above_Prompt_Are_Clamped()
    {
        var config = MakeConfig(inputPrice: 10m, cachedPrice: 1m);
        // 1M prompt, huge cached — clamp treats cached as 1M, uncached as 0.
        // Without the clamp, uncached would be negative.
        var cost = LlmForwarder.CalculateCostPoints(config, 1_000_000, 0, 999_999_999);
        // 1M / 1M * 1 = 1 yuan = 1000 points
        Assert.Equal(1000L, cost);
    }

    [Fact]
    public void Negative_Cached_Tokens_Are_Rejected()
    {
        // A negative cached count from upstream is treated as malformed data
        // and the entire call is billed 0 points rather than risk a credit
        // (cached bucket would otherwise offset the uncached prompt). This
        // is intentionally conservative — we'd rather under-bill than pay
        // the user for a broken upstream response.
        var config = MakeConfig(inputPrice: 10m, cachedPrice: 1m);
        var cost = LlmForwarder.CalculateCostPoints(config, 1_000_000, 0, -50);
        Assert.Equal(0L, cost);
    }

    [Fact]
    public void Cost_Rounds_Up_To_Whole_Points()
    {
        // Prices that produce a non-integer point amount must be rounded
        // UP (ceiling) so a non-zero usage always bills at least 1 point.
        // Forexample: 333_333 tokens * 1 元/1M = 0.333333... 元 = 333.333
        // points. AwayFromZero would round to 333 points (0.333 元) which
        // is close to zero cost; ceiling rounds up to 334 points (0.334 元).
        var config = MakeConfig(inputPrice: 1m, outputPrice: 3m, cachedPrice: 0.5m);
        var cost = LlmForwarder.CalculateCostPoints(config, 333_333, 0);
        Assert.Equal(334L, cost);
    }

    [Fact]
    public void Sub_Point_Usage_Still_Bills_One_Point()
    {
        // Any non-zero token consumption must cost at least 1 point, otherwise
        // tiny requests would round to zero and effectively be free.
        var config = MakeConfig(inputPrice: 0.0001m);
        // 1 token * 0.0001 元/1M * 1000 = 0.0000001 points before rounding.
        // Without ceiling, this would round to 0 — bug.
        var cost = LlmForwarder.CalculateCostPoints(config, 1, 0);
        Assert.Equal(1L, cost);
    }
}
