namespace Inkuso.Cloud.Core.Billing;

/// <summary>
/// Product-wide bounds for values that can be entered by an administrator.
/// These limits stay well below both Int64.MaxValue and JavaScript's maximum
/// safe integer, so API values can be represented exactly by the admin UI.
/// </summary>
public static class BillingLimits
{
    public const long MaxSingleCreditPoints = 1_000_000_000_000L;
    public const long MaxAccountBalancePoints = 5_000_000_000_000L;
    public const int MaxCodeUses = 1_000_000;
    public const int MinCodeLength = 4;
    public const int MaxCodeLength = 64;
    public const int MaxAdjustmentReasonLength = 500;

    public static string NormalizeCode(string? code) => (code ?? string.Empty).Trim();

    public static string? ValidateCode(string code)
    {
        if (code.Length < MinCodeLength || code.Length > MaxCodeLength)
            return $"Code must be between {MinCodeLength} and {MaxCodeLength} characters";

        if (code.Any(character =>
                !char.IsAsciiLetterOrDigit(character)
                && character is not '-' and not '_'))
            return "Code may contain only ASCII letters, digits, '-' and '_'";

        return null;
    }

    public static string? ValidatePointGrant(long points, bool allowZero)
    {
        var minimum = allowZero ? 0L : 1L;
        return points < minimum || points > MaxSingleCreditPoints
            ? $"Points must be between {minimum} and {MaxSingleCreditPoints}"
            : null;
    }

    public static string? ValidateMaxUses(int maxUses) =>
        maxUses < 1 || maxUses > MaxCodeUses
            ? $"MaxUses must be between 1 and {MaxCodeUses}"
            : null;
}
