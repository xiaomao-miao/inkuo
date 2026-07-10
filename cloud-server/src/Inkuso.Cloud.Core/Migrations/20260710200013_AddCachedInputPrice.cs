using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Inkuso.Cloud.Core.Migrations
{
    /// <inheritdoc />
    public partial class AddCachedInputPrice : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<long>(
                name: "CachedPromptTokens",
                table: "UsageRecords",
                type: "bigint",
                nullable: false,
                defaultValue: 0L);

            migrationBuilder.AddColumn<decimal>(
                name: "CachedInputPricePerMTokens",
                table: "ModelConfigs",
                type: "numeric(12,6)",
                precision: 12,
                scale: 6,
                nullable: false,
                defaultValue: 0m);

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000001"),
                column: "CachedInputPricePerMTokens",
                value: 0.1m);

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000002"),
                column: "CachedInputPricePerMTokens",
                value: 0.075m);

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000003"),
                column: "CachedInputPricePerMTokens",
                value: 1.25m);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "CachedPromptTokens",
                table: "UsageRecords");

            migrationBuilder.DropColumn(
                name: "CachedInputPricePerMTokens",
                table: "ModelConfigs");
        }
    }
}
