using Microsoft.AspNetCore.Authorization;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Inkuso.Cloud.Core.Data;

namespace Inkuso.Cloud.Api.Endpoints;

public static class Models
{
    public record ModelDto(string Id, string DisplayName, string ModelName, string Provider,
        decimal InputPricePerMTokens, decimal OutputPricePerMTokens, decimal CachedInputPricePerMTokens, string? Description);

    public static void MapModelsEndpoints(this WebApplication app)
    {
        var group = app.MapGroup("/v1").WithTags("models");

        group.MapGet("/models", [Authorize] async (AppDbContext db) =>
        {
            var models = await db.ModelConfigs
                .Where(m => m.Enabled)
                .OrderBy(m => m.SortOrder)
                .Select(m => new ModelDto(
                    m.Id.ToString(),
                    m.DisplayName,
                    m.ModelName,
                    m.UpstreamProvider,
                    m.InputPricePerMTokens,
                    m.OutputPricePerMTokens,
                    m.CachedInputPricePerMTokens,
                    m.Description))
                .ToListAsync();

            return Results.Ok(new { obj = "list", data = models });
        });
    }
}