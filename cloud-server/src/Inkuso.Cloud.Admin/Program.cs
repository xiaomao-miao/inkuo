using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;
using System.Text;
using Inkuso.Cloud.Admin.Auth;
using Inkuso.Cloud.Admin.Endpoints;
using Inkuso.Cloud.Admin.Middleware;
using Inkuso.Cloud.Core.Data;

var builder = WebApplication.CreateBuilder(args);

// Database
var connectionString = builder.Configuration.GetConnectionString("Postgres")
    ?? throw new InvalidOperationException("ConnectionStrings:Postgres is required");

builder.Services.AddDbContext<AppDbContext>(opt =>
    opt.UseNpgsql(connectionString)
       .ConfigureWarnings(w => w.Ignore(
           Microsoft.EntityFrameworkCore.Diagnostics.RelationalEventId.PendingModelChangesWarning)));

// JWT (separate audience from customer API)
var jwtSecret = builder.Configuration["Jwt:Secret"]
    ?? throw new InvalidOperationException("Jwt:Secret is required");
var jwtIssuer = builder.Configuration["Jwt:Issuer"] ?? "inkuo-cloud";
var adminAudience = builder.Configuration["Jwt:AdminAudience"] ?? "inkuo-admin";

builder.Services.AddSingleton<AdminJwtService>();

builder.Services.AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
    .AddJwtBearer(opt =>
    {
        opt.RequireHttpsMetadata = false; // dev convenience
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

// CORS for the React admin frontend (separate port 5174 in dev)
builder.Services.AddCors(opt => opt.AddDefaultPolicy(p =>
    p.AllowAnyHeader().AllowAnyMethod().AllowAnyOrigin()));

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

    // Seed default admin if none exists
    if (!db.AdminUsers.Any())
    {
        var seedUsername = app.Configuration["Admin:SeedUsername"] ?? "admin";
        var seedPassword = app.Configuration["Admin:SeedPassword"] ?? "admin123";
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

// Serve the built React admin SPA (production)
app.MapAdminSpa();

app.Run();