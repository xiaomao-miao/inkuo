using Inkuso.Cloud.Core.Data;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Inkuso.Cloud.Core.Migrations;

[DbContext(typeof(AppDbContext))]
[Migration("20260824170000_DurableBillingLifecycle")]
public sealed class DurableBillingLifecycle : Migration
{
    protected override void Up(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.AddColumn<long>(
            name: "DebtPoints",
            table: "Users",
            type: "bigint",
            nullable: false,
            defaultValue: 0L);

        migrationBuilder.AddColumn<bool>(
            name: "AdminSuspended",
            table: "Users",
            type: "boolean",
            nullable: false,
            defaultValue: false);

        // The previous binary represented debt only on UsageRecords and the
        // suspension flag. Recover outstanding amounts for accounts that are
        // still suspended; an operator who explicitly unsuspended an account
        // before this migration is treated as having forgiven that legacy debt.
        migrationBuilder.Sql("""
            UPDATE "Users" AS account
            SET "DebtPoints" = debt."OutstandingPoints"
            FROM (
                SELECT "UserId",
                       SUM(GREATEST("CostPoints" - COALESCE("ReservedPoints", 0), 0)) AS "OutstandingPoints"
                FROM "UsageRecords"
                WHERE "BillingStatus" = 'debt'
                GROUP BY "UserId"
            ) AS debt
            WHERE account."Id" = debt."UserId"
              AND account."IsSuspended" = TRUE;
            """);

        // A suspended account with no reconstructed billing debt represents a
        // manual/operator suspension and must remain banned after future credit.
        migrationBuilder.Sql("""
            UPDATE "Users"
            SET "AdminSuspended" = TRUE
            WHERE "IsSuspended" = TRUE AND "DebtPoints" = 0;
            """);

        migrationBuilder.AddColumn<decimal>(
            name: "CachedInputPricePerMTokensSnapshot",
            table: "UsageRecords",
            type: "numeric(12,6)",
            precision: 12,
            scale: 6,
            nullable: true);

        migrationBuilder.AddColumn<decimal>(
            name: "InputPricePerMTokensSnapshot",
            table: "UsageRecords",
            type: "numeric(12,6)",
            precision: 12,
            scale: 6,
            nullable: true);

        migrationBuilder.AddColumn<decimal>(
            name: "OutputPricePerMTokensSnapshot",
            table: "UsageRecords",
            type: "numeric(12,6)",
            precision: 12,
            scale: 6,
            nullable: true);

        migrationBuilder.AddColumn<string>(
            name: "BillingStatus",
            table: "WebSearchUsageRecords",
            type: "character varying(16)",
            maxLength: 16,
            nullable: false,
            defaultValue: "settled");

        migrationBuilder.AddColumn<long>(
            name: "CostPoints",
            table: "WebSearchUsageRecords",
            type: "bigint",
            nullable: false,
            defaultValue: 50L);

        migrationBuilder.AddColumn<string>(
            name: "RequestId",
            table: "WebSearchUsageRecords",
            type: "character varying(64)",
            maxLength: 64,
            nullable: true);

        migrationBuilder.AddColumn<long>(
            name: "ReservedPoints",
            table: "WebSearchUsageRecords",
            type: "bigint",
            nullable: true,
            defaultValue: 50L);

        migrationBuilder.CreateIndex(
            name: "IX_WebSearchUsageRecords_UserId_RequestId",
            table: "WebSearchUsageRecords",
            columns: new[] { "UserId", "RequestId" },
            unique: true);

        // Freeze the rate card for reservations created by an older binary
        // before this migration landed. Without this backfill a later admin
        // price edit would change the cost of an already accepted request.
        migrationBuilder.Sql("""
            UPDATE "UsageRecords" AS usage
            SET "InputPricePerMTokensSnapshot" = model."InputPricePerMTokens",
                "OutputPricePerMTokensSnapshot" = model."OutputPricePerMTokens",
                "CachedInputPricePerMTokensSnapshot" = model."CachedInputPricePerMTokens"
            FROM "ModelConfigs" AS model
            WHERE usage."ModelConfigId" = model."Id"
              AND (usage."InputPricePerMTokensSnapshot" IS NULL
                   OR usage."OutputPricePerMTokensSnapshot" IS NULL
                   OR usage."CachedInputPricePerMTokensSnapshot" IS NULL);
            """);
    }

    protected override void Down(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.DropIndex(
            name: "IX_WebSearchUsageRecords_UserId_RequestId",
            table: "WebSearchUsageRecords");
        migrationBuilder.DropColumn(name: "BillingStatus", table: "WebSearchUsageRecords");
        migrationBuilder.DropColumn(name: "CostPoints", table: "WebSearchUsageRecords");
        migrationBuilder.DropColumn(name: "RequestId", table: "WebSearchUsageRecords");
        migrationBuilder.DropColumn(name: "ReservedPoints", table: "WebSearchUsageRecords");
        migrationBuilder.DropColumn(name: "AdminSuspended", table: "Users");
        migrationBuilder.DropColumn(name: "DebtPoints", table: "Users");
        migrationBuilder.DropColumn(name: "CachedInputPricePerMTokensSnapshot", table: "UsageRecords");
        migrationBuilder.DropColumn(name: "InputPricePerMTokensSnapshot", table: "UsageRecords");
        migrationBuilder.DropColumn(name: "OutputPricePerMTokensSnapshot", table: "UsageRecords");
    }
}
