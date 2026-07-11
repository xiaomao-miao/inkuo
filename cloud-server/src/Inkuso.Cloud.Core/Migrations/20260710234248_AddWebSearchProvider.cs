using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Inkuso.Cloud.Core.Migrations
{
    /// <inheritdoc />
    public partial class AddWebSearchProvider : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.CreateTable(
                name: "WebSearchProviders",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    ProviderId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    DisplayName = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    UpstreamBaseUrl = table.Column<string>(type: "character varying(512)", maxLength: 512, nullable: true),
                    UpstreamApiKey = table.Column<string>(type: "text", nullable: true),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebSearchProviders", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "WebSearchUsageRecords",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    UserId = table.Column<Guid>(type: "uuid", nullable: false),
                    ProviderId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Query = table.Column<string>(type: "character varying(512)", maxLength: 512, nullable: false),
                    RecordedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebSearchUsageRecords", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WebSearchUsageRecords_Users_UserId",
                        column: x => x.UserId,
                        principalTable: "Users",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.InsertData(
                table: "WebSearchProviders",
                columns: new[] { "Id", "CreatedAt", "DisplayName", "Enabled", "ProviderId", "UpstreamApiKey", "UpstreamBaseUrl" },
                values: new object[] { new Guid("00000000-0000-0000-0002-000000000001"), new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), "百度百科", true, "baike", null, "https://appbuilder.baidu.com/v2/baike/lemma/get_content" });

            migrationBuilder.CreateIndex(
                name: "IX_WebSearchProviders_ProviderId",
                table: "WebSearchProviders",
                column: "ProviderId",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WebSearchUsageRecords_UserId_RecordedAt",
                table: "WebSearchUsageRecords",
                columns: new[] { "UserId", "RecordedAt" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebSearchProviders");

            migrationBuilder.DropTable(
                name: "WebSearchUsageRecords");
        }
    }
}
