using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;
using System.Text;
using Inkuso.Cloud.Api.Endpoints;
using Inkuso.Cloud.Core.Auth;
using Inkuso.Cloud.Core.Data;
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
    Secret = builder.Configuration["Jwt:Secret"] ?? throw new InvalidOperationException("Jwt:Secret is required"),
    Issuer = builder.Configuration["Jwt:Issuer"] ?? "inkuo-cloud",
    Audience = builder.Configuration["Jwt:Audience"] ?? "inkuo-desktop",
    AccessExpiryMinutes = builder.Configuration.GetValue("Jwt:AccessExpiryMinutes", 15),
    RefreshExpiryDays = builder.Configuration.GetValue("Jwt:RefreshExpiryDays", 30),
};

builder.Services.AddSingleton(jwtSettings);
builder.Services.AddScoped<JwtService>();

// Auth
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

builder.Services.AddCors(opt => opt.AddDefaultPolicy(p =>
    p.AllowAnyHeader().AllowAnyMethod().AllowAnyOrigin()));

var app = builder.Build();

// Auto-migrate on startup (dev convenience)
using (var scope = app.Services.CreateScope())
{
    var db = scope.ServiceProvider.GetRequiredService<AppDbContext>();
    db.Database.Migrate();
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