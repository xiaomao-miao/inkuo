using System.Text;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.AspNetCore.Http.Features;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;
using Inkuso.Cloud.Admin.Auth;
using Inkuso.Cloud.Admin.Endpoints;
using Inkuso.Cloud.Admin.Middleware;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Security;

var builder = WebApplication.CreateBuilder(args);

// Raise request-body limits so admins can upload multi-hundred-MiB Tauri
// installers via the Releases upload endpoint. Kestrel defaults to 30 MiB
// which is too small for a NSIS-bundled Windows installer that ships the
// WebView2 runtime.
builder.WebHost.ConfigureKestrel(opts =>
{
    opts.Limits.MaxRequestBodySize = 2L * 1024 * 1024 * 1024; // 2 GiB
});
builder.Services.Configure<FormOptions>(opts =>
{
    opts.MultipartBodyLengthLimit = 2L * 1024 * 1024 * 1024; // 2 GiB
    opts.ValueLengthLimit = int.MaxValue;
});

// Database
var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

builder.Services.AddDbContext<AppDbContext>(opt =>
    opt.UseNpgsql(connectionString)
       .ConfigureWarnings(w => w.Ignore(
           Microsoft.EntityFrameworkCore.Diagnostics.RelationalEventId.PendingModelChangesWarning)));

// JWT (separate audience from customer API)
// Require an explicit JWT secret; refuse to start with a placeholder or a key
// shorter than 32 bytes (HS256 minimum) so we don't ship a JWT stack that an
// attacker can brute-force offline.
var jwtSecret = builder.Configuration["Jwt:Secret"]
    ?? throw new InvalidOperationException("Jwt:Secret is required");
if (jwtSecret.Length < 32 || jwtSecret.StartsWith("change-me", StringComparison.OrdinalIgnoreCase))
    throw new InvalidOperationException(
        "Jwt:Secret must be at least 32 characters of random data and not a placeholder. "
      + "Generate one with `openssl rand -base64 48`.");
var jwtIssuer = builder.Configuration["Jwt:Issuer"] ?? "inkuo-cloud";
var adminAudience = builder.Configuration["Jwt:AdminAudience"] ?? "inkuo-admin";

builder.Services.AddSingleton<AdminJwtService>();

// At-rest protection for operator-supplied upstream API keys. Persisting
// the DataProtection key ring to a known location (override via
// DataProtection__KeyDir) lets the Admin and Api services share keys when
// they live on the same host — required because the Admin writes the
// protected ciphertext and the Api reads it.
var dpKeysDir = builder.Configuration["DataProtection:KeyDir"];
if (!string.IsNullOrWhiteSpace(dpKeysDir))
    builder.Services.AddDataProtection()
        .PersistKeysToFileSystem(new DirectoryInfo(dpKeysDir))
        .SetApplicationName("inkuo-cloud");
else
    builder.Services.AddDataProtection().SetApplicationName("inkuo-cloud");

builder.Services.AddSingleton<ISecretProtector, DataProtectionSecretProtector>();

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
            ValidIssuer = jwtIssuer,
            ValidAudience = adminAudience,
            IssuerSigningKey = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(jwtSecret)),
            ClockSkew = TimeSpan.FromMinutes(1),
        };
    });
builder.Services.AddAuthorization();

// Mirror the cloud-api project's JSON conventions so the admin SPA (which
// translates camelCase to snake_case in its axios interceptor) can talk to
// the minimal-API endpoints without 400s from missing-property validation.
// Without this, a payload like {"provider_id": "baike", ...} would not
// bind to a record `ProviderId` (case-sensitive by default).
builder.Services.ConfigureHttpJsonOptions(opt =>
{
    opt.SerializerOptions.PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower;
    opt.SerializerOptions.PropertyNameCaseInsensitive = true;
    opt.SerializerOptions.DictionaryKeyPolicy = System.Text.Json.JsonNamingPolicy.SnakeCaseLower;
});

// CORS for the React admin frontend.
// In production we should restrict this to the actual admin SPA origin;
// allow-listing via config lets us keep `AllowAnyOrigin` only in Development.
var allowedOrigins = builder.Configuration.GetSection("Admin:Cors:AllowedOrigins").Get<string[]>() ?? Array.Empty<string>();
builder.Services.AddCors(opt => opt.AddDefaultPolicy(p =>
{
    if (allowedOrigins.Length > 0)
        p.WithOrigins(allowedOrigins).AllowAnyHeader().AllowAnyMethod().AllowCredentials();
    else if (builder.Environment.IsDevelopment())
        p.AllowAnyHeader().AllowAnyMethod().AllowAnyOrigin();
    else
        // Lock down by default in non-Development environments when no allow-list
        // is configured so we don't ship a wildcard-CORS service to production.
        p.WithOrigins("https://localhost").AllowAnyHeader().AllowAnyMethod();
}));

// Swagger for development
builder.Services.AddEndpointsApiExplorer();
builder.Services.AddSwaggerGen(c =>
{
    c.SwaggerDoc("v1", new() { Title = "inkuo Cloud Admin API", Version = "v1" });
    c.AddSecurityDefinition("Bearer", new Microsoft.OpenApi.Models.OpenApiSecurityScheme
    {
        Description = "JWT token from /api/auth/login",
        Name = "Authorization",
        In = Microsoft.OpenApi.Models.ParameterLocation.Header,
        Type = Microsoft.OpenApi.Models.SecuritySchemeType.Http,
        Scheme = "bearer",
        BearerFormat = "JWT"
    });
    c.AddSecurityRequirement(new Microsoft.OpenApi.Models.OpenApiSecurityRequirement
    {
        {
            new Microsoft.OpenApi.Models.OpenApiSecurityScheme
            {
                Reference = new Microsoft.OpenApi.Models.OpenApiReference
                {
                    Type = Microsoft.OpenApi.Models.ReferenceType.SecurityScheme,
                    Id = "Bearer"
                }
            },
            Array.Empty<string>()
        }
    });
});

var app = builder.Build();

// Auto-migrate + seed default admin user
using (var scope = app.Services.CreateScope())
{
    var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
    db.Database.Migrate();

    // Seed default admin if none exists.
    // We require Admin:SeedUsername/SeedPassword to be set explicitly so the
    // service refuses to start with a known default credential in production.
    if (!db.AdminUsers.Any())
    {
        var seedUsername = app.Configuration["Admin:SeedUsername"]
            ?? throw new InvalidOperationException("Admin:SeedUsername is required when no admin user exists");
        var seedPassword = app.Configuration["Admin:SeedPassword"]
            ?? throw new InvalidOperationException("Admin:SeedPassword is required when no admin user exists");
        if (seedPassword.Length < 12)
            throw new InvalidOperationException(
                "Admin:SeedPassword must be at least 12 characters. "
              + "Generate one with `openssl rand -base64 24`.");
        db.AdminUsers.Add(new Inkuso.Cloud.Core.Entities.AdminUser
        {
            Username = seedUsername,
            PasswordHash = BCrypt.Net.BCrypt.HashPassword(seedPassword),
            Role = "superadmin",
            Enabled = true,
            CreatedAt = DateTime.UtcNow,
        });
        db.SaveChanges();
        app.Logger.LogWarning("Seeded default admin user: {Username} (CHANGE PASSWORD IMMEDIATELY)", seedUsername);
    }
}

app.UseCors();
app.UseAuthentication();
app.UseAuthorization();

// Swagger only in dev (not behind reverse proxy anyway)
if (app.Environment.IsDevelopment())
{
    app.UseSwagger();
    app.UseSwaggerUI();
}

app.MapGet("/health", () => Results.Ok(new { status = "ok", service = "inkuo-cloud-admin" }));

// Admin API endpoints (all under /api)
app.MapAdminAuthEndpoints();
app.MapDashboardEndpoints();
app.MapAdminUsersEndpoints();
app.MapAdminPlansEndpoints();
app.MapAdminModelConfigsEndpoints();
app.MapAdminWebSearchProvidersEndpoints();
app.MapAdminInviteCodesEndpoints();
app.MapAdminRedemptionCodesEndpoints();
app.MapAdminUsageEndpoints();
app.MapAdminReleasesEndpoints();

// Serve the built React admin SPA (production)
app.MapAdminSpa();

app.Run();