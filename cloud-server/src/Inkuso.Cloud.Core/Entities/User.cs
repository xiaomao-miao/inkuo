namespace Inkuso.Cloud.Core.Entities;

public class User
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Email { get; set; } = string.Empty;
    public string PasswordHash { get; set; } = string.Empty;
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public string? InviteCodeUsed { get; set; }

    // Account currency is points (1 元 = 1000 点, 1 点 = 0.001 元).
    // Positive = available credit, negative = debt carried after a failed deduction.
    // BalancePoints + ReservedPoints == total entitlements a user can spend.
    public long BalancePoints { get; set; } = 0;
    public long ReservedPoints { get; set; } = 0;
    public bool IsSuspended { get; set; } = false; // set true after a deduction failure; blocks new requests until admin unblocks
}
