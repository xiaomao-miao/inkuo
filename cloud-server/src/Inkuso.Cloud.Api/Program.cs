using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;
using System.Text;
using Inkuso.Cloud.Api.Endpoints;
using Inkuso.Cloud.Core.Auth;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Security;
using Inkuso.Cloud.Core.Upstream;
using Inkuso.Cloud.Core.Billing;

var builder = WebApplication.CreateBuilder(args);

// The API accepts compact JSON/SSE requests, not file uploads. Bound request
// buffering before Chat parses the body so an authenticated client cannot make
// the process retain the default 30 MiB per request under concurrency.
builder.WebHost.ConfigureKestrel(options =>
{
    options.Limits.MaxRequestBodySize = 8L * 1024 * 1024;
});

// Database
var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

builder.Services.AddDbContext<AppDbContext>(opt => opt.UseNpgsql(connectionString));

// JWT
var jwtSettings = new JwtSettings
{
    Secret = builder.Configuration["Jwt:Secret"]
        ?? throw new InvalidOperationException("Jwt:Secret is required"),
    Issuer = builder.Configuration["Jwt:Issuer"] ?? "inkuo-cloud",
    Audience = builder.Configuration["Jwt:Audience"] ?? "inkuo-desktop",
    AccessExpiryMinutes = builder.Configuration.GetValue("Jwt:AccessExpiryMinutes", 15),
    RefreshExpiryDays = builder.Configuration.GetValue("Jwt:RefreshExpiryDays", 30),
};
if (string.IsNullOrWhiteSpace(jwtSettings.Issuer)
    || string.IsNullOrWhiteSpace(jwtSettings.Audience))
    throw new InvalidOperationException("Jwt:Issuer and Jwt:Audience must not be blank");
if (jwtSettings.AccessExpiryMinutes is < 1 or > 1440)
    throw new InvalidOperationException("Jwt:AccessExpiryMinutes must be between 1 and 1440");
if (jwtSettings.RefreshExpiryDays is < 1 or > 365)
    throw new InvalidOperationException("Jwt:RefreshExpiryDays must be between 1 and 365");
// Refuse placeholders and keys below HS256's 256-bit minimum.
if (CredentialPolicy.IsWeakSecret(jwtSettings.Secret))
{
    throw new InvalidOperationException(
        "Jwt:Secret must be at least 32 UTF-8 bytes of random data and not a placeholder. "
      + "Generate one with `openssl rand -base64 48`.");
}

builder.Services.AddSingleton(jwtSettings);
builder.Services.AddScoped<JwtService>();

// At-rest protection for operator-supplied upstream API keys (model
// providers + web-search providers). Must share the DataProtection key
// ring with the Admin service — see the matching block in
// Inkuso.Cloud.Admin/Program.cs.
var apiDpKeysDir = builder.Configuration["DataProtection:KeyDir"];
if (string.IsNullOrWhiteSpace(apiDpKeysDir) && !builder.Environment.IsDevelopment())
    throw new InvalidOperationException(
        "DataProtection:KeyDir is required outside Development and must be shared with Admin.");
var apiDataProtection = builder.Services.AddDataProtection()
    .SetApplicationName("inkuo-cloud");
if (!string.IsNullOrWhiteSpace(apiDpKeysDir))
    apiDataProtection.PersistKeysToFileSystem(new DirectoryInfo(apiDpKeysDir));

builder.Services.AddSingleton<ISecretProtector>(services =>
    new DataProtectionSecretProtector(
        services.GetRequiredService<IDataProtectionProvider>()
            .CreateProtector(DataProtectionSecretProtector.Purpose)));

// Auth
builder.Services.AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
    .AddJwtBearer(opt =>
    {
        // Only disable HTTPS metadata in Development; production must validate
        // the IdP signing keys over TLS to avoid MITM token-stripping attacks.
        opt.RequireHttpsMetadata = !builder.Environment.IsDevelopment();
        opt.TokenValidationParameters = new TokenValidationParameters
        {
            ValidateIssuer = true,
            ValidateAudience = true,
            ValidateLifetime = true,
            ValidateIssuerSigningKey = true,
            ValidIssuer = jwtSettings.Issuer,
            ValidAudience = jwtSettings.Audience,
            IssuerSigningKey = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(jwtSettings.Secret)),
            ClockSkew = TimeSpan.FromMinutes(1),
        };
    });
builder.Services.AddAuthorization();

builder.Services.ConfigureHttpJsonOptions(opt =>
{
    // Emit snake_case in JSON to match the desktop client's Rust struct
    // definitions and to stay consistent with the database column naming.
    opt.SerializerOptions.PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower;
    opt.SerializerOptions.DictionaryKeyPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower;
});

// HTTP client for upstream forwarding
builder.Services.AddHttpClient("upstream");
builder.Services.AddHttpClient("upstream-search");

// LLM forwarder
builder.Services.AddScoped<LlmForwarder>();
builder.Services.AddScoped<BillingLedger>();
builder.Services.AddScoped<WebSearchLedger>();
// Web search forwarder: scoped (mirrors LlmForwarder) so it can pick up
// the latest provider config from the DbContext on every call without
// a stale-cache surprise when the operator pastes a new API key.
builder.Services.AddScoped<WebSearchForwarder>();

// CORS: lock down by default in production. The desktop client does not
// need cross-origin requests to talk to this service, so we expose CORS only
// when explicitly allow-listed via configuration (e.g. for the admin SPA
// sharing this host or for local browser-based tooling in Development).
var apiCorsOrigins = builder.Configuration.GetSection("Api:Cors:AllowedOrigins").Get<string[]>() ?? Array.Empty<string>();
builder.Services.AddCors(opt => opt.AddDefaultPolicy(p =>
{
    if (apiCorsOrigins.Length > 0)
        p.WithOrigins(apiCorsOrigins).AllowAnyHeader().AllowAnyMethod().AllowCredentials();
    else if (builder.Environment.IsDevelopment())
        p.AllowAnyHeader().AllowAnyMethod().AllowAnyOrigin();
    // No CORS policy in production unless explicitly configured — the
    // desktop client talks to this service via Tauri's native HTTP stack
    // (which is not subject to browser CORS), so allowing any origin in
    // production would only widen the attack surface for no benefit.
}));

var app = builder.Build();

// Resolve once at startup so a broken Data Protection registration fails the
// deployment before the first operator saves or first customer uses a key.
_ = app.Services.GetRequiredService<ISecretProtector>();

// Auto-migrate on startup (dev convenience). Migration failures are logged and
// fail fast. In production the migrate step should be a separate init job so a
// broken migration cannot bring a partially upgraded API into rotation. Production deploys are
// expected to set Database__AutoMigrate=false and run `dotnet ef database
// update` as a Kubernetes Job / DB migration step before rolling the API.
if (builder.Configuration.GetValue("Database:AutoMigrate", builder.Environment.IsDevelopment()))
{
    using (var scope = app.Services.CreateScope())
    {
        var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
        var logger = scope.ServiceProvider.GetRequiredService<ILoggerFactory>().CreateLogger("Inkuso.Cloud.Migration");
        try
        {
            db.Database.Migrate();
        }
        catch (Exception ex)
        {
            logger.LogCritical(ex, "Database migration failed at startup; aborting");
            throw;
        }
    }
}

// Do the credential upgrade in both Api and Admin. This closes the startup
// ordering window where Api could begin forwarding with legacy plaintext rows
// while Admin (the original backfill owner) was still waiting to start.
using (var scope = app.Services.CreateScope())
{
    var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
    var protector = scope.ServiceProvider.GetRequiredService<ISecretProtector>();
    var protectedSecretCount = await LegacySecretBackfill.ProtectAsync(db, protector);
    if (protectedSecretCount > 0)
        app.Logger.LogInformation(
            "Protected {SecretCount} legacy upstream credential row(s) at rest",
            protectedSecretCount);
}

app.UseCors();
app.UseAuthentication();
app.UseAuthorization();

app.MapGet("/health", () => Results.Ok(new { status = "ok", service = "inkuo-cloud-api" }));

app.MapAuthEndpoints();
app.MapModelsEndpoints();
app.MapChatEndpoints();
app.MapAccountEndpoints();
app.MapRedeemEndpoints();
app.MapWebSearchEndpoints();

app.Run();
