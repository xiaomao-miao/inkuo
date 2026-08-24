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
        migrationBuilder.UpdateData(
            table: "InviteCodes",
            keyColumn: "Id",
            keyValue: 1,
            columns: new[] { "Enabled", "FreePoints" },
            values: new object[] { false, 0L });

        migrationBuilder.UpdateData(
            table: "RedemptionCodes",
            keyColumn: "Id",
            keyValue: 1,
            columns: new[] { "CreditPoints", "Enabled" },
            values: new object[] { 0L, false });
    }

    protected override void Down(MigrationBuilder migrationBuilder)
    {
        // Security migration: intentionally irreversible. Rolling back the
        // schema must never silently re-enable credentials and credit codes
        // that are publicly known from repository history.
    }
}
