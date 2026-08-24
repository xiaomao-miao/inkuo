using Inkuso.Cloud.Core.Data;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Inkuso.Cloud.Core.Migrations;

[DbContext(typeof(AppDbContext))]
[Migration("20260824130000_HardenBillingIdempotency")]
public sealed class HardenBillingIdempotency : Migration
{
    protected override void Up(MigrationBuilder migrationBuilder)
    {
        // Old builds could create duplicate audit rows while falling back after
        // a failed settlement. Keep every row, but detach the idempotency key
        // from all but the earliest row before enforcing uniqueness.
        migrationBuilder.Sql("""
            WITH ranked AS (
                SELECT "Id",
                       ROW_NUMBER() OVER (
                           PARTITION BY "UserId", "RequestId"
                           ORDER BY "RecordedAt", "Id") AS rn
                FROM "UsageRecords"
                WHERE "RequestId" IS NOT NULL
            )
            UPDATE "UsageRecords" AS u
            SET "RequestId" = NULL
            FROM ranked
            WHERE u."Id" = ranked."Id" AND ranked.rn > 1;
            """);

        migrationBuilder.AlterColumn<string>(
            name: "RequestId",
            table: "UsageRecords",
            type: "character varying(64)",
            maxLength: 64,
            nullable: true,
            oldClrType: typeof(string),
            oldType: "text",
            oldNullable: true);

        migrationBuilder.CreateIndex(
            name: "IX_UsageRecords_UserId_RequestId",
            table: "UsageRecords",
            columns: new[] { "UserId", "RequestId" },
            unique: true);
    }

    protected override void Down(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.DropIndex(
            name: "IX_UsageRecords_UserId_RequestId",
            table: "UsageRecords");

        migrationBuilder.AlterColumn<string>(
            name: "RequestId",
            table: "UsageRecords",
            type: "text",
            nullable: true,
            oldClrType: typeof(string),
            oldType: "character varying(64)",
            oldMaxLength: 64,
            oldNullable: true);
    }
}
