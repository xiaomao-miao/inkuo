using Inkuso.Cloud.Core.Security;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public sealed class CredentialPolicyTests
{
    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("short")]
    [InlineData("replace-with-openssl-rand-base64-48-output")]
    [InlineData("prefix-change_me_in_production-padding-padding")]
    [InlineData("your-secret-goes-here-with-extra-padding")]
    public void IsWeakSecret_rejects_short_or_placeholder_values(string? secret)
    {
        Assert.True(CredentialPolicy.IsWeakSecret(secret));
    }

    [Fact]
    public void IsWeakSecret_accepts_a_long_random_value()
    {
        Assert.False(CredentialPolicy.IsWeakSecret(
            "dc41G8iTQ8RW0bQd5k1bXcYbQ8xvLnB1JxVZQm5wX0I="));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("elevenchars")]
    public void ValidatePassword_rejects_missing_or_short_values(string? password)
    {
        Assert.NotNull(CredentialPolicy.ValidatePassword(password));
    }

    [Fact]
    public void ValidatePassword_rejects_more_than_72_utf8_bytes()
    {
        // 25 CJK characters are 75 bytes in UTF-8.
        Assert.NotNull(CredentialPolicy.ValidatePassword(new string('密', 25)));
    }

    [Theory]
    [InlineData("a-secure-passphrase")]
    [InlineData("十二个字符的安全密码短语")]
    public void ValidatePassword_accepts_valid_values(string password)
    {
        Assert.Null(CredentialPolicy.ValidatePassword(password));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("ab")]
    [InlineData("name with spaces")]
    public void ValidateAdminUsername_rejects_invalid_values(string? username)
    {
        Assert.NotNull(CredentialPolicy.ValidateAdminUsername(username));
    }

    [Theory]
    [InlineData("admin")]
    [InlineData("ops@example")]
    [InlineData("运营管理员")]
    public void ValidateAdminUsername_accepts_bounded_non_whitespace_values(string username)
    {
        Assert.Null(CredentialPolicy.ValidateAdminUsername(username));
    }
}
