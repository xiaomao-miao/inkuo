// <copyright file="LlmForwarderCostTests.cs" company="inkuo">
// Unit tests for LlmForwarder.CalculateCost — the cost math is the single
// source of truth for what users are billed, so we want regression coverage
// on edge cases (zero tokens, cached >= prompt, decimal rounding).
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
        Assert.Equal(0m, LlmForwarder.CalculateCost(config, 0, 0, 0));
    }

    [Fact]
    public void Input_Only_Bills_At_Input_Price()
    {
        var config = MakeConfig(inputPrice: 10m);
        // 1_000_000 tokens * 10 yuan / 1M = 10 yuan = 1000 cents
        Assert.Equal(1000m, LlmForwarder.CalculateCost(config, 1_000_000, 0));
    }

    [Fact]
    public void Output_Only_Bills_At_Output_Price()
    {
        var config = MakeConfig(outputPrice: 25m);
        Assert.Equal(2500m, LlmForwarder.CalculateCost(config, 0, 1_000_000));
    }

    [Fact]
    public void Cached_Tokens_Subtract_From_Prompt_Total()
    {
        var config = MakeConfig(inputPrice: 10m, cachedPrice: 1m);
        // 1_000_000 prompt total, 600_000 cached, 400_000 uncached
        // cached:   600_000 / 1M * 1   = 0.6 yuan
        // input:    400_000 / 1M * 10  = 4 yuan
        // output:   0
        // total:    4.6 yuan = 460 cents
        var cost = LlmForwarder.CalculateCost(config, 1_000_000, 0, 600_000);
        Assert.Equal(460m, cost);
    }

    [Fact]
    public void Cached_Tokens_Above_Prompt_Are_Clamped()
    {
        var config = MakeConfig(inputPrice: 10m, cachedPrice: 1m);
        // 1_000_000 prompt, 999_999_999 cached — defensive clamp should
        // treat cached as 1_000_000 and uncached as 0 (otherwise we'd
        // over-bill because cached price < uncached price).
        var cost = LlmForwarder.CalculateCost(config, 1_000_000, 0, 999_999_999);
        Assert.Equal(100m, cost); // 1M / 1M * 1 = 1 yuan = 100 cents
    }

    [Fact]
    public void Negative_Cached_Tokens_Are_Clamped_To_Zero()
    {
        var config = MakeConfig(inputPrice: 10m, cachedPrice: 1m);
        // Negative cached shouldn't subtract from prompt; the full prompt
        // is billed at the normal input price.
        var cost = LlmForwarder.CalculateCost(config, 1_000_000, 0, -50);
        Assert.Equal(1000m, cost); // 1M / 1M * 10 = 10 yuan = 1000 cents
    }

    [Fact]
    public void Cost_Is_Rounded_To_Cents()
    {
        // Use prices that produce a non-integer yuan amount; the function
        // must round to cents to keep the integer cents column in DB tidy.
        var config = MakeConfig(inputPrice: 1m, outputPrice: 3m, cachedPrice: 0.5m);
        var cost = LlmForwarder.CalculateCost(config, 333_333, 0);
        // 333_333 / 1_000_000 * 1 = 0.333333 yuan -> 33.3333 cents -> 33.33
        Assert.Equal(33.33m, cost);
    }
}
