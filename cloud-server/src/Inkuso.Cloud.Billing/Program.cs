using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Billing.Services;
using Inkuso.Cloud.Core.Billing;
using Inkuso.Cloud.Core.Data;

var builder = WebApplication.CreateBuilder(args);

var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

builder.Services.AddDbContext<AppDbContext>(opt => opt.UseNpgsql(connectionString)
       .ConfigureWarnings(w => w.Ignore(
           Microsoft.EntityFrameworkCore.Diagnostics.RelationalEventId.PendingModelChangesWarning)));
builder.Services.AddScoped<BillingLedger>();
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
