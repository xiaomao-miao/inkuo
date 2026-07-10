using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Inkuso.Cloud.Core.Migrations
{
    /// <inheritdoc />
    public partial class ModelPricePer1M : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.RenameColumn(
                name: "OutputPricePer1kTokens",
                table: "ModelConfigs",
                newName: "OutputPricePerMTokens");

            migrationBuilder.RenameColumn(
                name: "InputPricePer1kTokens",
                table: "ModelConfigs",
                newName: "InputPricePerMTokens");

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000001"),
                columns: new[] { "InputPricePerMTokens", "OutputPricePerMTokens" },
                values: new object[] { 1.0m, 2.0m });

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000002"),
                columns: new[] { "InputPricePerMTokens", "OutputPricePerMTokens" },
                values: new object[] { 0.15m, 0.6m });

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000003"),
                columns: new[] { "InputPricePerMTokens", "OutputPricePerMTokens" },
                values: new object[] { 2.5m, 10.0m });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.RenameColumn(
                name: "OutputPricePerMTokens",
                table: "ModelConfigs",
                newName: "OutputPricePer1kTokens");

            migrationBuilder.RenameColumn(
                name: "InputPricePerMTokens",
                table: "ModelConfigs",
                newName: "InputPricePer1kTokens");

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000001"),
                columns: new[] { "InputPricePer1kTokens", "OutputPricePer1kTokens" },
                values: new object[] { 0.001m, 0.002m });

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000002"),
                columns: new[] { "InputPricePer1kTokens", "OutputPricePer1kTokens" },
                values: new object[] { 0.00015m, 0.0006m });

            migrationBuilder.UpdateData(
                table: "ModelConfigs",
                keyColumn: "Id",
                keyValue: new Guid("00000000-0000-0000-0001-000000000003"),
                columns: new[] { "InputPricePer1kTokens", "OutputPricePer1kTokens" },
                values: new object[] { 0.0025m, 0.01m });
        }
    }
}
