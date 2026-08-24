namespace Inkuso.Cloud.Core.Entities;

public class User
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Email { get; set; } = string.Empty;
    public string PasswordHash { get; set; } = string.Empty;
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public string? InviteCodeUsed { get; set; }

    // Account currency is points (1 元 = 1000 点, 1 点 = 0.001 元).
    // BalancePoints is total unspent credit. ReservedPoints is the frozen subset,
    // so available credit is BalancePoints - ReservedPoints and the invariant is
    // 0 <= ReservedPoints <= BalancePoints.
    public long BalancePoints { get; set; } = 0;
    public long ReservedPoints { get; set; } = 0;
    public long DebtPoints { get; set; } = 0;
    // Manual operator bans are tracked separately so paying a billing debt
    // cannot accidentally clear an abuse/security suspension.
    public bool AdminSuspended { get; set; } = false;
    public bool IsSuspended { get; set; } = false; // set true after a deduction failure; blocks new requests until admin unblocks
}
