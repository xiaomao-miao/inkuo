namespace Inkuso.Cloud.Admin.Middleware;

/// <summary>
/// Serves the built React admin SPA from wwwroot/.
/// Any GET that does not match an API endpoint or a static asset
/// falls back to index.html so the SPA router can handle it.
/// </summary>
public static class SpaFallbackExtensions
{
    public static WebApplication MapAdminSpa(this WebApplication app)
    {
        var spaRoot = Path.Combine(app.Environment.WebRootPath ?? "wwwroot", "admin");

        // Serve static assets directly
        app.UseStaticFiles(new StaticFileOptions
        {
            FileProvider = new Microsoft.Extensions.FileProviders.PhysicalFileProvider(spaRoot),
            RequestPath = "",
        });

        // SPA fallback for client-side routes
        app.MapFallback(async ctx =>
        {
            var indexPath = Path.Combine(spaRoot, "index.html");
            if (!File.Exists(indexPath))
            {
                ctx.Response.StatusCode = 404;
                await ctx.Response.WriteAsync(
                    "Admin SPA not built. Run `pnpm build` in cloud-server/admin-frontend/ " +
                    "or use development mode via `pnpm dev`.");
                return;
            }
            ctx.Response.ContentType = "text/html; charset=utf-8";
            await ctx.Response.SendFileAsync(indexPath);
        });

        return app;
    }
}