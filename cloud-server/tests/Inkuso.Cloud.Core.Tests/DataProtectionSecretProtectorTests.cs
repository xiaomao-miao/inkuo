// <copyright file="DataProtectionSecretProtectorTests.cs" company="inkuo">
// Unit tests for DataProtectionSecretProtector — verifies the at-rest
// protection contract for the operator-supplied upstream API keys:
//   - protect/unprotect round-trips for non-empty values
//   - null/empty is preserved as a no-op
//   - legacy plaintext rows (no `dp:` prefix) still read back unchanged so a
//     rolling migration doesn't break existing data
// </copyright>

using Microsoft.AspNetCore.DataProtection;
using Inkuso.Cloud.Core.Security;
using Xunit;

namespace Inkuso.Cloud.Core.Tests;

public class DataProtectionSecretProtectorTests
{
    private static ISecretProtector NewProtector()
    {
        // Ephemeral key ring so each test is independent and we don't need
        // any on-disk key material to make the test green.
        var provider = DataProtectionProvider.Create("inkuo-cloud-test");
        var protector = provider.CreateProtector("inkuo-cloud-test:secrets");
        return new DataProtectionSecretProtector(protector);
    }

    [Fact]
    public void Protect_Null_Returns_Null()
    {
        var p = NewProtector();
        Assert.Null(p.Protect(null));
    }

    [Fact]
    public void Protect_Empty_Returns_Empty()
    {
        var p = NewProtector();
        Assert.Equal(string.Empty, p.Protect(string.Empty));
    }

    [Fact]
    public void Protect_Then_Unprotect_Round_Trips()
    {
        var p = NewProtector();
        const string secret = "sk-upstream-abcdef0123456789";
        var protectedValue = p.Protect(secret);
        Assert.NotEqual(secret, protectedValue);
        Assert.StartsWith("dp:", protectedValue);
        Assert.Equal(secret, p.Unprotect(protectedValue));
        Assert.True(p.IsProtected(protectedValue));
        Assert.False(p.IsProtected(secret));
    }

    [Fact]
    public void Protected_Payload_Is_Not_Plaintext()
    {
        var p = NewProtector();
        var protectedValue = p.Protect("hunter2");
        // A blunt sanity check: the protected payload must not contain the
        // plaintext substring. AES-CBC + HMAC-SHA256 (DataProtection's
        // default) makes this overwhelmingly unlikely even for a 7-char
        // input, so a regression here would mean the encryption broke.
        Assert.DoesNotContain("hunter2", protectedValue);
    }

    [Fact]
    public void Legacy_Plaintext_Rows_Are_Returned_As_Is()
    {
        var p = NewProtector();
        // Rows written before DataProtection was rolled out still look like
        // raw API keys (no `dp:` prefix). They must keep reading back so a
        // rolling migration doesn't 500 the admin panel.
        const string legacy = "sk-legacy-plaintext-key";
        Assert.Equal(legacy, p.Unprotect(legacy));
    }

    [Fact]
    public void Unprotect_Null_Returns_Null()
    {
        var p = NewProtector();
        Assert.Null(p.Unprotect(null));
        Assert.False(p.IsProtected(null));
    }
}
