using Inkuso.Cloud.Core.Data;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Inkuso.Cloud.Core.Migrations;

[DbContext(typeof(AppDbContext))]
[Migration("20260824160000_DisablePublicSeedCodes")]
public sealed class DisablePublicSeedCodes : Migration
{
    protected override void Up(MigrationBuilder migrationBuilder)
    {
        // These values were published in the repository and deployment docs.
        // Leaving either row enabled would give anyone a predictable path to
        // mint operator-funded credit after a fresh or upgraded deployment.
        // This security migration is intentionally hand-written rather than
        // scaffolded, so it has no generated TargetModel designer. UpdateData
        // asks EF to infer column mappings from that TargetModel and therefore
        // fails before emitting SQL. Explicit PostgreSQL SQL is unambiguous and
        // keeps the operation valid for both fresh databases and upgrades.
        migrationBuilder.Sql(
            """
            UPDATE "InviteCodes"
            SET "Enabled" = FALSE, "FreePoints" = 0
            WHERE "Id" = 1;

            UPDATE "RedemptionCodes"
            SET "CreditPoints" = 0, "Enabled" = FALSE
            WHERE "Id" = 1;
            """);
    }

    protected override void Down(MigrationBuilder migrationBuilder)
    {
        // Security migration: intentionally irreversible. Rolling back the
        // schema must never silently re-enable credentials and credit codes
        // that are publicly known from repository history.
    }
}
