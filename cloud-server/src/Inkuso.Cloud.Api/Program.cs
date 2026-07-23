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

var builder = WebApplication.CreateBuilder(args);

// Database
var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

builder.Services.AddDbContext<AppDbContext>(opt =>
    opt.UseNpgsql(connectionString)
       // Allow auto-migrate to apply when seed data drift is detected
       // without bringing down the whole service.
       .ConfigureWarnings(w => w.Ignore(
           Microsoft.EntityFrameworkCore.Diagnostics.RelationalEventId.PendingModelChangesWarning)));

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
// Refuse to start with a placeholder or too-short HS256 key so we can't be
// brute-forced offline if the secret leaks through a misconfigured deploy.
// Covers: "change-me" (the old pattern), "replace-with" (the current
// .env.example), "your-" (a common onboarding placeholder), and anything
// shorter than 32 chars (below HS256's 256-bit minimum).
static bool IsWeakSecret(string s) =>
    s.Length < 32
    || s.StartsWith("change-me",    StringComparison.OrdinalIgnoreCase)
    || s.StartsWith("replace-with", StringComparison.OrdinalIgnoreCase)
    || s.StartsWith("your-",        StringComparison.OrdinalIgnoreCase);

if (IsWeakSecret(jwtSettings.Secret))
{
    throw new InvalidOperationException(
        "Jwt:Secret must be at least 32 characters of random data and not a placeholder. "
      + "Generate one with `openssl rand -base64 48`.");
}

builder.Services.AddSingleton(jwtSettings);
builder.Services.AddScoped<JwtService>();

// At-rest protection for operator-supplied upstream API keys (model
// providers + web-search providers). Must share the DataProtection key
// ring with the Admin service — see the matching block in
// Inkuso.Cloud.Admin/Program.cs.
var apiDpKeysDir = builder.Configuration["DataProtection:KeyDir"];
if (!string.IsNullOrWhiteSpace(apiDpKeysDir))
    builder.Services.AddDataProtection()
        .PersistKeysToFileSystem(new DirectoryInfo(apiDpKeysDir))
        .SetApplicationName("inkuo-cloud");
else
    builder.Services.AddDataProtection().SetApplicationName("inkuo-cloud");

builder.Services.AddSingleton<ISecretProtector, DataProtectionSecretProtector>();

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

// Auto-migrate on startup (dev convenience). We swallow migration failures
// and log them rather than crash-looping the container: in production the
// migrate step should be a separate init job that fails fast, so a broken
// migration shouldn't take the API out of rotation. Production deploys are
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