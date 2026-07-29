using System;
using Microsoft.EntityFrameworkCore.Migrations;
using Npgsql.EntityFrameworkCore.PostgreSQL.Metadata;

#nullable disable

#pragma warning disable CA1814 // Prefer jagged arrays over multidimensional

namespace Inkuso.Cloud.Core.Migrations
{
    /// <inheritdoc />
    public partial class InitialCreate : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.CreateTable(
                name: "AdminUsers",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    Username = table.Column<string>(type: "text", nullable: false),
                    PasswordHash = table.Column<string>(type: "text", nullable: false),
                    Role = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    LastLoginAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_AdminUsers", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "InviteCodes",
                columns: table => new
                {
                    Id = table.Column<int>(type: "integer", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    Code = table.Column<string>(type: "text", nullable: false),
                    FreePoints = table.Column<long>(type: "bigint", nullable: false),
                    MaxUses = table.Column<int>(type: "integer", nullable: false),
                    UsedCount = table.Column<int>(type: "integer", nullable: false),
                    ExpiresAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_InviteCodes", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "ModelConfigs",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    UpstreamProvider = table.Column<string>(type: "text", nullable: false),
                    UpstreamBaseUrl = table.Column<string>(type: "text", nullable: false),
                    UpstreamApiKey = table.Column<string>(type: "text", nullable: false),
                    ModelName = table.Column<string>(type: "text", nullable: false),
                    DisplayName = table.Column<string>(type: "text", nullable: false),
                    Description = table.Column<string>(type: "text", nullable: true),
                    InputPricePerMTokens = table.Column<decimal>(type: "numeric(12,6)", precision: 12, scale: 6, nullable: false),
                    OutputPricePerMTokens = table.Column<decimal>(type: "numeric(12,6)", precision: 12, scale: 6, nullable: false),
                    CachedInputPricePerMTokens = table.Column<decimal>(type: "numeric(12,6)", precision: 12, scale: 6, nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false),
                    SortOrder = table.Column<int>(type: "integer", nullable: false),
                    MaxOutputTokens = table.Column<int>(type: "integer", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_ModelConfigs", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "Plans",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    Name = table.Column<string>(type: "text", nullable: false),
                    MonthlyPricePoints = table.Column<long>(type: "bigint", nullable: false),
                    MonthlyTokenLimit = table.Column<long>(type: "bigint", nullable: false),
                    OverageInputPricePer1k = table.Column<decimal>(type: "numeric(12,6)", precision: 12, scale: 6, nullable: false),
                    OverageOutputPricePer1k = table.Column<decimal>(type: "numeric(12,6)", precision: 12, scale: 6, nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Plans", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "Releases",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    Version = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Channel = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    Platform = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    Architecture = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    FileName = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    FileSizeBytes = table.Column<long>(type: "bigint", nullable: false),
                    Sha256 = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    StoragePath = table.Column<string>(type: "character varying(512)", maxLength: 512, nullable: false),
                    DownloadUrl = table.Column<string>(type: "character varying(512)", maxLength: 512, nullable: false),
                    ReleaseNotes = table.Column<string>(type: "text", nullable: true),
                    IsLatest = table.Column<bool>(type: "boolean", nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    CreatedByAdminId = table.Column<Guid>(type: "uuid", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Releases", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "Users",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    Email = table.Column<string>(type: "text", nullable: false),
                    PasswordHash = table.Column<string>(type: "text", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    InviteCodeUsed = table.Column<string>(type: "text", nullable: true),
                    BalancePoints = table.Column<long>(type: "bigint", nullable: false),
                    ReservedPoints = table.Column<long>(type: "bigint", nullable: false),
                    IsSuspended = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Users", x => x.Id);
                });

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
                name: "RedemptionCodes",
                columns: table => new
                {
                    Id = table.Column<int>(type: "integer", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    Code = table.Column<string>(type: "text", nullable: false),
                    PlanId = table.Column<Guid>(type: "uuid", nullable: true),
                    CreditPoints = table.Column<long>(type: "bigint", nullable: false),
                    MaxUses = table.Column<int>(type: "integer", nullable: false),
                    UsedCount = table.Column<int>(type: "integer", nullable: false),
                    ExpiresAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_RedemptionCodes", x => x.Id);
                    table.ForeignKey(
                        name: "FK_RedemptionCodes_Plans_PlanId",
                        column: x => x.PlanId,
                        principalTable: "Plans",
                        principalColumn: "Id");
                });

            migrationBuilder.CreateTable(
                name: "RefreshTokens",
                columns: table => new
                {
                    Id = table.Column<int>(type: "integer", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    Jti = table.Column<string>(type: "text", nullable: false),
                    UserId = table.Column<Guid>(type: "uuid", nullable: false),
                    ExpiresAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Revoked = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_RefreshTokens", x => x.Id);
                    table.ForeignKey(
                        name: "FK_RefreshTokens_Users_UserId",
                        column: x => x.UserId,
                        principalTable: "Users",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "Subscriptions",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    UserId = table.Column<Guid>(type: "uuid", nullable: false),
                    PlanId = table.Column<Guid>(type: "uuid", nullable: false),
                    StartedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    ExpiresAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Status = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    CreatedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Subscriptions", x => x.Id);
                    table.ForeignKey(
                        name: "FK_Subscriptions_Plans_PlanId",
                        column: x => x.PlanId,
                        principalTable: "Plans",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
                    table.ForeignKey(
                        name: "FK_Subscriptions_Users_UserId",
                        column: x => x.UserId,
                        principalTable: "Users",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "UsageRecords",
                columns: table => new
                {
                    Id = table.Column<Guid>(type: "uuid", nullable: false),
                    UserId = table.Column<Guid>(type: "uuid", nullable: false),
                    ModelConfigId = table.Column<Guid>(type: "uuid", nullable: false),
                    PromptTokens = table.Column<long>(type: "bigint", nullable: false),
                    CachedPromptTokens = table.Column<long>(type: "bigint", nullable: false),
                    CompletionTokens = table.Column<long>(type: "bigint", nullable: false),
                    CostPoints = table.Column<long>(type: "bigint", nullable: false),
                    ReservedPoints = table.Column<long>(type: "bigint", nullable: true),
                    RecordedAt = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    RequestId = table.Column<string>(type: "text", nullable: true),
                    BillingStatus = table.Column<string>(type: "character varying(16)", maxLength: 16, nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_UsageRecords", x => x.Id);
                    table.ForeignKey(
                        name: "FK_UsageRecords_ModelConfigs_ModelConfigId",
                        column: x => x.ModelConfigId,
                        principalTable: "ModelConfigs",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
                    table.ForeignKey(
                        name: "FK_UsageRecords_Users_UserId",
                        column: x => x.UserId,
                        principalTable: "Users",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Restrict);
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
                table: "InviteCodes",
                columns: new[] { "Id", "Code", "CreatedAt", "Enabled", "ExpiresAt", "FreePoints", "MaxUses", "UsedCount" },
                values: new object[] { 1, "INKUO2026", new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), true, null, 5000L, 9999, 0 });

            migrationBuilder.InsertData(
                table: "ModelConfigs",
                columns: new[] { "Id", "CachedInputPricePerMTokens", "CreatedAt", "Description", "DisplayName", "Enabled", "InputPricePerMTokens", "MaxOutputTokens", "ModelName", "OutputPricePerMTokens", "SortOrder", "UpstreamApiKey", "UpstreamBaseUrl", "UpstreamProvider" },
                values: new object[,]
                {
                    { new Guid("00000000-0000-0000-0001-000000000001"), 0.1m, new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), null, "DeepSeek-V3", true, 1.0m, 4096, "deepseek-chat", 2.0m, 1, "", "https://api.deepseek.com", "deepseek" },
                    { new Guid("00000000-0000-0000-0001-000000000002"), 0.075m, new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), null, "GPT-4o Mini", true, 0.15m, 4096, "gpt-4o-mini", 0.6m, 2, "", "https://api.openai.com/v1", "openai" },
                    { new Guid("00000000-0000-0000-0001-000000000003"), 1.25m, new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), null, "GPT-4o", true, 2.5m, 4096, "gpt-4o", 10.0m, 3, "", "https://api.openai.com/v1", "openai" }
                });

            migrationBuilder.InsertData(
                table: "Plans",
                columns: new[] { "Id", "CreatedAt", "Enabled", "MonthlyPricePoints", "MonthlyTokenLimit", "Name", "OverageInputPricePer1k", "OverageOutputPricePer1k" },
                values: new object[,]
                {
                    { new Guid("00000000-0000-0000-0000-000000000001"), new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), true, 0L, 500000L, "Free", 0.002m, 0.004m },
                    { new Guid("00000000-0000-0000-0000-000000000002"), new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), true, 29000L, 5000000L, "Plus", 0.002m, 0.004m },
                    { new Guid("00000000-0000-0000-0000-000000000003"), new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), true, 99000L, 25000000L, "Pro", 0.0015m, 0.003m },
                    { new Guid("00000000-0000-0000-0000-000000000004"), new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), true, 299000L, 100000000L, "Max", 0.001m, 0.002m }
                });

            migrationBuilder.InsertData(
                table: "RedemptionCodes",
                columns: new[] { "Id", "Code", "CreatedAt", "CreditPoints", "Enabled", "ExpiresAt", "MaxUses", "PlanId", "UsedCount" },
                values: new object[] { 1, "WELCOME-5000", new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), 5000L, true, null, 9999, null, 0 });

            migrationBuilder.InsertData(
                table: "WebSearchProviders",
                columns: new[] { "Id", "CreatedAt", "DisplayName", "Enabled", "ProviderId", "UpstreamApiKey", "UpstreamBaseUrl" },
                values: new object[] { new Guid("00000000-0000-0000-0002-000000000001"), new DateTime(2025, 1, 1, 0, 0, 0, 0, DateTimeKind.Utc), "百度百科", true, "baike", null, "https://appbuilder.baidu.com/v2/baike/lemma/get_content" });

            migrationBuilder.CreateIndex(
                name: "IX_AdminUsers_Username",
                table: "AdminUsers",
                column: "Username",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_InviteCodes_Code",
                table: "InviteCodes",
                column: "Code",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_RedemptionCodes_Code",
                table: "RedemptionCodes",
                column: "Code",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_RedemptionCodes_PlanId",
                table: "RedemptionCodes",
                column: "PlanId");

            migrationBuilder.CreateIndex(
                name: "IX_RefreshTokens_Jti",
                table: "RefreshTokens",
                column: "Jti",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_RefreshTokens_UserId",
                table: "RefreshTokens",
                column: "UserId");

            migrationBuilder.CreateIndex(
                name: "IX_Releases_CreatedAt",
                table: "Releases",
                column: "CreatedAt");

            migrationBuilder.CreateIndex(
                name: "IX_Releases_Enabled_IsLatest",
                table: "Releases",
                columns: new[] { "Enabled", "IsLatest" });

            migrationBuilder.CreateIndex(
                name: "IX_Releases_Platform_Architecture_Channel_Version",
                table: "Releases",
                columns: new[] { "Platform", "Architecture", "Channel", "Version" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_Subscriptions_PlanId",
                table: "Subscriptions",
                column: "PlanId");

            migrationBuilder.CreateIndex(
                name: "IX_Subscriptions_UserId",
                table: "Subscriptions",
                column: "UserId");

            migrationBuilder.CreateIndex(
                name: "IX_UsageRecords_ModelConfigId",
                table: "UsageRecords",
                column: "ModelConfigId");

            migrationBuilder.CreateIndex(
                name: "IX_UsageRecords_UserId_RecordedAt",
                table: "UsageRecords",
                columns: new[] { "UserId", "RecordedAt" });

            migrationBuilder.CreateIndex(
                name: "IX_Users_Email",
                table: "Users",
                column: "Email",
                unique: true);

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
                name: "AdminUsers");

            migrationBuilder.DropTable(
                name: "InviteCodes");

            migrationBuilder.DropTable(
                name: "RedemptionCodes");

            migrationBuilder.DropTable(
                name: "RefreshTokens");

            migrationBuilder.DropTable(
                name: "Releases");

            migrationBuilder.DropTable(
                name: "Subscriptions");

            migrationBuilder.DropTable(
                name: "UsageRecords");

            migrationBuilder.DropTable(
                name: "WebSearchProviders");

            migrationBuilder.DropTable(
                name: "WebSearchUsageRecords");

            migrationBuilder.DropTable(
                name: "Plans");

            migrationBuilder.DropTable(
                name: "ModelConfigs");

            migrationBuilder.DropTable(
                name: "Users");
        }
    }
}
