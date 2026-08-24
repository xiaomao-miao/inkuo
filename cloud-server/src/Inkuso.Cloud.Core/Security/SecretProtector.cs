// <copyright file="SecretProtector.cs" company="inkuo">
// At-rest protection for operator-supplied upstream credentials.
//
// Background: the V1 schema stores `UpstreamApiKey` as plaintext, which
// means a DB dump or SQL injection leaks every LLM / web-search provider
// key. We wrap the value with ASP.NET Core DataProtection (authenticated
// encryption with AES-CBC + HMAC-SHA256) and persist the protected payload
// as base64. The keys are kept in the OS-level key-ring by default
// (per-user on Linux/Mac, DPAPI on Windows); in production we recommend
// pointing PersistKeysToFileSystem at a dedicated volume.
//
// Wire format: `dp:<base64-protected-payload>`. The `dp:` prefix lets us
// distinguish protected values from legacy plaintext rows during a
// rolling migration without a schema change.
//
// Notes:
//  - `Protect` is a no-op for null/empty so callers can pass `string?`
//    straight through without a guard at every callsite.
//  - `Unprotect` falls back to returning the original value if it does
//    not carry the `dp:` prefix — this is what makes the rolling
//    migration non-breaking for rows that were written before this
//    service existed.
// </copyright>

using System.Text;
using Microsoft.AspNetCore.DataProtection;

namespace Inkuso.Cloud.Core.Security;

public interface ISecretProtector
{
    bool IsProtected(string? value);
    string? Protect(string? plaintext);
    string? Unprotect(string? protectedValue);
}

public class DataProtectionSecretProtector(IDataProtector protector) : ISecretProtector
{
    public const string Purpose = "inkuo-cloud:upstream-credentials:v1";
    private const string Prefix = "dp:";

    public bool IsProtected(string? value) =>
        !string.IsNullOrEmpty(value)
        && value.StartsWith(Prefix, StringComparison.Ordinal);

    public string? Protect(string? plaintext)
    {
        if (string.IsNullOrEmpty(plaintext)) return plaintext;
        // IDataProtector.Protect only takes byte[] in .NET 10; the string
        // overload is a DataProtectionCommonExtensions extension method that
        // some trimming/AOT paths drop, so we drive the byte[] API directly to
        // stay portable across hosts (ASP.NET, tests, native AOT).
        var bytes = Encoding.UTF8.GetBytes(plaintext);
        var payload = protector.Protect(bytes);
        return Prefix + Convert.ToBase64String(payload);
    }

    public string? Unprotect(string? protectedValue)
    {
        if (string.IsNullOrEmpty(protectedValue)) return protectedValue;
        if (!IsProtected(protectedValue))
            // Legacy plaintext row — return as-is so we don't break reads
            // while a rolling migration is in progress.
            return protectedValue;

        var b64 = protectedValue[Prefix.Length..];
        var bytes = Convert.FromBase64String(b64);
        var plain = protector.Unprotect(bytes);
        return Encoding.UTF8.GetString(plain);
    }
}
