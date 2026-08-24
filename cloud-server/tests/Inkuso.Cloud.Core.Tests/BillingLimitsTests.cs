using Inkuso.Cloud.Core.Billing;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class BillingLimitsTests
{
    [Theory]
    [InlineData("  BETA_2026  ", "BETA_2026")]
    [InlineData(null, "")]
    public void NormalizeCode_Trims_External_Whitespace(string? raw, string expected)
    {
        Assert.Equal(expected, BillingLimits.NormalizeCode(raw));
    }

    [Theory]
    [InlineData("ABC")]
    [InlineData("HAS SPACE")]
    [InlineData("CODE!")]
    public void ValidateCode_Rejects_Unsafe_Values(string code)
    {
        Assert.NotNull(BillingLimits.ValidateCode(code));
    }

    [Fact]
    public void Point_And_Use_Bounds_Reject_Values_Outside_The_Admin_Envelope()
    {
        Assert.Null(BillingLimits.ValidatePointGrant(0, allowZero: true));
        Assert.NotNull(BillingLimits.ValidatePointGrant(0, allowZero: false));
        Assert.NotNull(BillingLimits.ValidatePointGrant(
            BillingLimits.MaxSingleCreditPoints + 1,
            allowZero: true));
        Assert.NotNull(BillingLimits.ValidateMaxUses(0));
        Assert.NotNull(BillingLimits.ValidateMaxUses(BillingLimits.MaxCodeUses + 1));
    }

    [Fact]
    public void Limits_Are_Exactly_Representable_In_JavaScript()
    {
        const long javascriptMaxSafeInteger = 9_007_199_254_740_991L;

        Assert.InRange(BillingLimits.MaxSingleCreditPoints, 1L, javascriptMaxSafeInteger);
        Assert.InRange(BillingLimits.MaxAccountBalancePoints, 1L, javascriptMaxSafeInteger);
        Assert.True(BillingLimits.MaxSingleCreditPoints < BillingLimits.MaxAccountBalancePoints);
    }
}
