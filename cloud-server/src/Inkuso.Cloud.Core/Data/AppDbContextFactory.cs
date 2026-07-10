using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;

namespace Inkuso.Cloud.Core.Data;

// Used by `dotnet ef` so design-time tools don't need a real connection string / JWT secret.
public class AppDbContextFactory : IDesignTimeDbContextFactory<AppDbContext>
{
    public AppDbContext CreateDbContext(string[] args)
    {
        var options = new DbContextOptionsBuilder<AppDbContext>()
            .UseNpgsql("Host=localhost;Database=inkuo;Username=postgres;Password=postgres")
            .Options;
        return new AppDbContext(options);
    }
}
