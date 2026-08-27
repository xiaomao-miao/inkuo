using System.Text;

namespace Inkuso.Cloud.Core.Security;

/// <summary>
/// Central validation for credentials accepted by the cloud services.
/// Keeping this policy in Core prevents the API, Admin, and Billing processes
/// from drifting into accepting different sample values or weak passwords.
/// </summary>
public static class CredentialPolicy
{
    public const int MinimumSecretBytes = 32;
    public const int MinimumPasswordCharacters = 12;
    public const int MaximumBcryptPasswordBytes = 72;
    public const int MinimumAdminUsernameCharacters = 3;
    public const int MaximumAdminUsernameCharacters = 64;

    private static readonly string[] PlaceholderFragments =
    [
        "change-me",
        "change_me",
        "replace-with",
        "replace_with",
        "your-",
        "your-secret",
        "your_secret",
        "example-secret",
        "example_secret",
    ];

    public static bool IsWeakSecret(string? secret, int minimumBytes = MinimumSecretBytes)
    {
        if (string.IsNullOrWhiteSpace(secret))
            return true;

        var candidate = secret.Trim();
        if (Encoding.UTF8.GetByteCount(candidate) < minimumBytes)
            return true;

        return PlaceholderFragments.Any(fragment =>
            candidate.Contains(fragment, StringComparison.OrdinalIgnoreCase));
    }

    /// <summary>
    /// Returns an English validation message, or <see langword="null"/> when
    /// the password is safe to pass to BCrypt. BCrypt only considers the first
    /// 72 UTF-8 bytes, so rejecting longer input prevents two visibly different
    /// passwords from authenticating as the same credential.
    /// </summary>
    public static string? ValidatePassword(string? password)
    {
        if (string.IsNullOrWhiteSpace(password)
            || password.Length < MinimumPasswordCharacters)
        {
            return $"Password must be at least {MinimumPasswordCharacters} characters";
        }

        if (Encoding.UTF8.GetByteCount(password) > MaximumBcryptPasswordBytes)
        {
            return $"Password must be at most {MaximumBcryptPasswordBytes} UTF-8 bytes";
        }

        return null;
    }

    public static string? ValidateAdminUsername(string? username)
    {
        var candidate = username?.Trim() ?? string.Empty;
        if (candidate.Length is < MinimumAdminUsernameCharacters or > MaximumAdminUsernameCharacters)
        {
            return $"Username must be between {MinimumAdminUsernameCharacters} and "
                 + $"{MaximumAdminUsernameCharacters} characters";
        }
        if (candidate.Any(character => char.IsWhiteSpace(character) || char.IsControl(character)))
            return "Username must not contain whitespace or control characters";
        return null;
    }
}
