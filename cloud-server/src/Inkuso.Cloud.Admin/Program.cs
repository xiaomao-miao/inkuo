using System.Text;
using System.Security.Claims;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;
using Inkuso.Cloud.Admin.Auth;
using Inkuso.Cloud.Admin.Endpoints;
using Inkuso.Cloud.Admin.Middleware;
using Inkuso.Cloud.Core.Data;
using Inkuso.Cloud.Core.Security;

var builder = WebApplication.CreateBuilder(args);

// Database
var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

builder.Services.AddDbContext<AppDbContext>(opt => opt.UseNpgsql(connectionString));

// JWT (separate audience from customer API)
// Require an explicit JWT secret; refuse to start with a placeholder or a key
// shorter than 32 bytes (HS256 minimum) so we don't ship a JWT stack that an
// attacker can brute-force offline.
var jwtSecret = builder.Configuration["Jwt:Secret"]
    ?? throw new InvalidOperationException("Jwt:Secret is required");
if (CredentialPolicy.IsWeakSecret(jwtSecret))
    throw new InvalidOperationException(
        "Jwt:Secret must be at least 32 UTF-8 bytes of random data and not a placeholder. "
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
if (string.IsNullOrWhiteSpace(dpKeysDir) && !builder.Environment.IsDevelopment())
    throw new InvalidOperationException(
        "DataProtection:KeyDir is required outside Development and must be shared with Api.");
var adminDataProtection = builder.Services.AddDataProtection()
    .SetApplicationName("inkuo-cloud");
if (!string.IsNullOrWhiteSpace(dpKeysDir))
    adminDataProtection.PersistKeysToFileSystem(new DirectoryInfo(dpKeysDir));

builder.Services.AddSingleton<ISecretProtector>(services =>
    new DataProtectionSecretProtector(
        services.GetRequiredService<IDataProtectionProvider>()
            .CreateProtector(DataProtectionSecretProtector.Purpose)));

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
        opt.Events = new JwtBearerEvents
        {
            OnTokenValidated = async context =>
            {
                var subject = context.Principal?.FindFirst(ClaimTypes.NameIdentifier)?.Value
                              ?? context.Principal?.FindFirst("sub")?.Value;
                var tokenRole = context.Principal?.FindFirst(ClaimTypes.Role)?.Value;
                var tokenType = context.Principal?.FindFirst("type")?.Value;
                if (!Guid.TryParse(subject, out var adminId) || tokenType != "admin")
                {
                    context.Fail("Invalid admin token claims");
                    return;
                }

                // A signed token must not outlive account deletion, disabling,
                // or a role change. Requiring a fresh login also prevents a
                // demoted superadmin from retaining privileged claims for the
                // remainder of the token's lifetime.
                var db = context.HttpContext.RequestServices.GetRequiredService<AppDbContext>();
                var current = await db.AdminUsers.AsNoTracking()
                    .Where(user => user.Id == adminId && user.Enabled)
                    .Select(user => new { user.Role })
                    .FirstOrDefaultAsync(context.HttpContext.RequestAborted);
                if (current is null || !string.Equals(current.Role, tokenRole, StringComparison.Ordinal))
                    context.Fail("Admin account is no longer active with this role");
            },
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

// Safe defaults for both SPAs and JSON endpoints. The admin UI uses inline
// style attributes (Ant Design), hence `unsafe-inline` is limited to styles;
// executable scripts remain same-origin only.
app.Use(async (context, next) =>
{
    context.Response.Headers.XContentTypeOptions = "nosniff";
    context.Response.Headers.XFrameOptions = "DENY";
    context.Response.Headers["Referrer-Policy"] = "no-referrer";
    if (!app.Environment.IsDevelopment())
        context.Response.Headers.ContentSecurityPolicy =
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; "
          + "img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; "
          + "object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'";
    context.Response.Headers["Permissions-Policy"] =
        "camera=(), microphone=(), geolocation=(), payment=()";
    await next();
});

// Fail fast if the protector cannot be constructed; waiting until the first
// Admin write would turn a deployment mistake into a runtime credential outage.
_ = app.Services.GetRequiredService<ISecretProtector>();

// Auto-migrate + seed default admin user
using (var scope = app.Services.CreateScope())
{
    var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
    db.Database.Migrate();

    // Upgrade credentials from installations that pre-date Data Protection.
    // This is deliberately idempotent and runs before the HTTP listener starts;
    // never log credential values or protected payloads.
    var protector = scope.ServiceProvider.GetRequiredService<ISecretProtector>();
    var protectedSecretCount = await LegacySecretBackfill.ProtectAsync(db, protector);
    if (protectedSecretCount > 0)
        app.Logger.LogInformation(
            "Protected {SecretCount} legacy upstream credential row(s) at rest",
            protectedSecretCount);

    // Seed default admin if none exists.
    // We require Admin:SeedUsername/SeedPassword to be set explicitly so the
    // service refuses to start with a known default credential in production.
    if (!db.AdminUsers.Any())
    {
        var seedUsername = app.Configuration["Admin:SeedUsername"]?.Trim()
            ?? throw new InvalidOperationException("Admin:SeedUsername is required when no admin user exists");
        var seedPassword = app.Configuration["Admin:SeedPassword"]
            ?? throw new InvalidOperationException("Admin:SeedPassword is required when no admin user exists");
        var seedPasswordError = CredentialPolicy.ValidatePassword(seedPassword);
        var seedUsernameError = CredentialPolicy.ValidateAdminUsername(seedUsername);
        if (seedUsernameError is not null)
            throw new InvalidOperationException($"Admin:SeedUsername is invalid: {seedUsernameError}");
        if (seedPasswordError is not null)
            throw new InvalidOperationException(
                $"Admin:SeedPassword is invalid: {seedPasswordError}. "
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
