namespace Inkuso.Cloud.Admin.Middleware;

/// <summary>
/// Wires the three public-facing surfaces of the admin service:
///   1. <c>wwwroot/</c>          — generic static (anything not matched below)
///   2. <c>wwwroot/marketing/</c> — public landing page served from <c>/</c>
///   3. <c>wwwroot/admin/</c>     — operator React SPA served from <c>/admin</c>
///                                  (any unknown sub-path falls back to index.html)
/// Installer downloads are deliberately handled by the releases endpoint so
/// disabling a release also revokes its URL; the storage directory is never
/// mounted as a public static-file root.
/// </summary>
public static class SpaFallbackExtensions
{
    public static WebApplication MapAdminSpa(this WebApplication app)
    {
        var webRoot = app.Environment.WebRootPath ?? "wwwroot";
        var marketingRoot = webRoot; // marketing site lives at wwwroot/ (root)
        var adminRoot = Path.Combine(webRoot, "admin");
        // 1. Generic static files (favicon, marketing assets, etc.).
        app.UseStaticFiles(new StaticFileOptions
        {
            FileProvider = new Microsoft.Extensions.FileProviders.PhysicalFileProvider(webRoot),
            RequestPath = "",
            OnPrepareResponse = context =>
            {
                var fileName = context.File.Name;
                context.Context.Response.Headers.CacheControl =
                    fileName.Equals("index.html", StringComparison.OrdinalIgnoreCase)
                        ? "no-cache"
                        : context.Context.Request.Path.StartsWithSegments("/assets")
                          || context.Context.Request.Path.StartsWithSegments("/admin/assets")
                            ? "public,max-age=31536000,immutable"
                            : "public,max-age=3600";
            },
        });

        // 2. Admin SPA: any GET under /admin that didn't match a static file
        //    falls back to the SPA's index.html so React Router can handle it.
        app.Map("/admin", adminApp =>
        {
            adminApp.Run(async ctx =>
            {
                var path = ctx.Request.Path.Value ?? "";
                // Only fall back for GET/HEAD on routes that aren't real assets.
                if (!HttpMethods.IsGet(ctx.Request.Method) && !HttpMethods.IsHead(ctx.Request.Method))
                {
                    ctx.Response.StatusCode = 405;
                    return;
                }

                var indexPath = Path.Combine(adminRoot, "index.html");
                if (!File.Exists(indexPath))
                {
                    ctx.Response.StatusCode = 404;
                    await ctx.Response.WriteAsync(
                        "Admin SPA not built. Run `pnpm build:deploy` in cloud-server/admin-frontend/.");
                    return;
                }

                // Serve index.html for client-side routes (anything without a
                // file extension). Real asset paths (e.g. /admin/assets/x.js)
                // are served by UseStaticFiles above because they exist on disk.
                var lastSegment = path.TrimStart('/').Split('/').LastOrDefault() ?? "";
                if (lastSegment.Contains('.'))
                {
                    ctx.Response.StatusCode = 404;
                    return;
                }

                ctx.Response.ContentType = "text/html; charset=utf-8";
                ctx.Response.Headers.CacheControl = "no-cache";
                await ctx.Response.SendFileAsync(indexPath);
            });
        });

        // 3. Marketing site fallback. The root must serve the marketing
        //    landing page. Any unknown path (no extension) also falls back
        //    to index.html — the marketing site is single-page so this is
        //    mostly a defensive default.
        app.MapFallback(async ctx =>
        {
            var path = ctx.Request.Path.Value ?? "/";

            // Only handle GET/HEAD. Anything else gets a 405 from routing.
            if (!HttpMethods.IsGet(ctx.Request.Method) && !HttpMethods.IsHead(ctx.Request.Method))
            {
                ctx.Response.StatusCode = 405;
                return;
            }

            // Anything that looks like a real file (has an extension) is a
            // genuine 404 — UseStaticFiles already tried to serve it.
            var lastSegment = path.TrimStart('/').Split('/').LastOrDefault() ?? "";
            if (lastSegment.Contains('.'))
            {
                ctx.Response.StatusCode = 404;
                return;
            }

            // Otherwise serve the marketing index.html.
            var indexPath = Path.Combine(marketingRoot, "index.html");
            if (!File.Exists(indexPath))
            {
                // Fall back to a tiny placeholder so a freshly-built admin
                // image that forgot to bake in the marketing site still
                // returns something useful (and points operators at the
                // admin panel) instead of a blank 404.
                ctx.Response.StatusCode = 200;
                ctx.Response.ContentType = "text/html; charset=utf-8";
                await ctx.Response.WriteAsync(
                    "<!DOCTYPE html><html><head><meta charset=\"utf-8\">" +
                    "<title>inkuo</title></head><body style=\"font-family:sans-serif;max-width:680px;margin:80px auto;line-height:1.6\">" +
                    "<h1>inkuo</h1>" +
                    "<p>The marketing site hasn't been built yet.</p>" +
                    "<p>Build it with <code>pnpm --dir cloud-server/marketing-frontend build:deploy</code> " +
                    "or rebuild the Docker image.</p>" +
                    "<p>Operators: <a href=\"/admin\">/admin</a></p>" +
                    "</body></html>");
                return;
            }

            ctx.Response.ContentType = "text/html; charset=utf-8";
            ctx.Response.Headers.CacheControl = "no-cache";
            await ctx.Response.SendFileAsync(indexPath);
        });

        return app;
    }
}
