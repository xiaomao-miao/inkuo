using Inkuso.Cloud.Core.Upstream;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class SseUsageParserTests
{
    [Fact]
    public void Complete_Usage_Is_Parsed_And_Cache_Is_Clamped()
    {
        const string line = "data: {\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"prompt_tokens_details\":{\"cached_tokens\":99}}}";

        Assert.True(SseUsageParser.TryParseLine(line, out var usage));
        Assert.Equal(12, usage.PromptTokens);
        Assert.Equal(7, usage.CompletionTokens);
        Assert.Equal(12, usage.CachedPromptTokens);
    }

    [Theory]
    [InlineData("data: {\"usage\":{}}")]
    [InlineData("data: {\"usage\":{\"prompt_tokens\":12}}")]
    [InlineData("data: {\"usage\":{\"prompt_tokens\":-1,\"completion_tokens\":2}}")]
    [InlineData("data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":\"2\"}}")]
    [InlineData("data: [DONE]")]
    [InlineData("data: not-json")]
    public void Incomplete_Or_Invalid_Usage_Uses_Fallback_Path(string line)
    {
        Assert.False(SseUsageParser.TryParseLine(line, out _));
    }
}
