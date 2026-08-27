using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Billing.Services;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Security;

var builder = WebApplication.CreateBuilder(args);

var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

var adminToken = builder.Configuration["Admin:Token"]
    ?? throw new InvalidOperationException("Admin:Token is required");
if (CredentialPolicy.IsWeakSecret(adminToken)
    || adminToken.StartsWith("dev-", StringComparison.OrdinalIgnoreCase))
{
    throw new InvalidOperationException(
        "Admin:Token must be at least 32 UTF-8 bytes of random data and not a placeholder. "
      + "Generate one with `openssl rand -base64 32`.");
}

builder.Services.AddDbContext<AppDbContext>(opt => opt.UseNpgsql(connectionString));
builder.Services.AddScoped<BillingLedger>();
builder.Services.AddScoped<WebSearchLedger>();
builder.Services.AddSingleton(new BillingAdminSettings(adminToken));
builder.Services.AddHostedService<ReconciliationWorker>();

var app = builder.Build();

using (var scope = app.Services.CreateScope())
{
    var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
    db.Database.Migrate();
}

app.MapGet("/health", () => Results.Ok(new { status = "ok", service = "inkuo-cloud-billing" }));

app.MapAdminEndpoints();

app.Run();
