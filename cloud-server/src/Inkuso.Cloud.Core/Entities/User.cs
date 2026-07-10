namespace Inkuso.Cloud.Core.Entities;

public class User
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Email { get; set; } = string.Empty;
    public string PasswordHash { get; set; } = string.Empty;
    public DateTime CreatedAt { get; set; } = DateTime.UtcNow;
    public string? InviteCodeUsed { get; set; }
    public decimal BalanceCents { get; set; } = 0; // positive = credit, negative = debit
}