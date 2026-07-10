# 部署指南 (Deployment Guide)

本文档覆盖两个部署面:

1. **inkuo Cloud Server** (C# ASP.NET Core) — 部署在一台云服务器上, 负责鉴权、计费、LLM 转发。
2. **inkuo 桌面端** (Tauri + React) — 用户本地运行, 编译成可执行文件分发。

---

## 一、部署 Cloud Server

### 前置要求

- 一台 Linux 云服务器 (Ubuntu 22.04+ / Debian 12+ 都行)
- 至少 2 vCPU / 2 GB RAM (空闲时消耗 < 200MB)
- 公网 IP + 域名 (用于 HTTPS 终结)
- 已安装 Docker + Docker Compose:
  ```bash
  curl -fsSL https://get.docker.com | sh
  sudo apt install docker-compose-plugin
  ```

### 0. 三个服务的端口

| 服务 | 端口 | 说明 |
|---|---|---|
| postgres | 5432 | 数据库 (一般只让内网访问) |
| api | 8080 | 桌面端用的 REST API |
| billing | 8081 | 后台 ReconciliationWorker + 老的 admin 端点 (X-Admin-Token) |
| **admin** | **8082** | **新的 admin Web UI (React SPA + /api/* 端点)** |

生产环境建议只把 8080 和 8082 暴露给外网 (分别对应桌面端和运营人员), 5432 和 8081 走内网或者不开端口。

### 1. 拉代码 & 配置

```bash
git clone https://github.com/your-org/inkuo.git
cd inkuo/cloud-server
cp .env.example .env
```

编辑 `.env`, **必须**修改以下几项:

```bash
# Postgres 密码 (避免使用默认值)
POSTGRES_PASSWORD=<强随机密码>

# JWT 签名密钥 (≥ 32 字符随机)
JWT_SECRET=$(openssl rand -base64 48)

# Admin 接口 token (后台脚本调用)
ADMIN_TOKEN=$(openssl rand -hex 32)
```

启动 stack:

```bash
docker compose --env-file .env up -d --build
```

启动后会自动:
- 启动 PostgreSQL, 跑 EF Core 迁移
- 启动 Api (端口 8080) 和 Billing (端口 8081)
- 注入种子数据: 4 个套餐 + 邀请码 `INKUO2026` + 3 个模型占位

### 2. 配置生产 HTTPS

最简单方案: 用 Caddy / nginx 反代。

```nginx
# /etc/nginx/sites-available/inkuo-cloud
server {
    listen 443 ssl http2;
    server_name cloud.inkuo.com;

    ssl_certificate /etc/letsencrypt/live/cloud.inkuo.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/cloud.inkuo.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE streaming 需要关掉 buffer
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 24h;
    }
}
```

### 3. 配置上游 LLM 密钥

**推荐方式: 用 admin Web UI** (端口 8082)

1. 浏览器打开 `https://admin.inkuo.com` (或本地 `http://localhost:8082`)
2. 用 `.env` 里的 `ADMIN_SEED_USERNAME` / `ADMIN_SEED_PASSWORD` 登录
3. 进入 "**模型配置**" 页面
4. 点击 "**查看完整 API Key**" 按钮 (默认是 masked 状态)
5. 点击每行的 "**编辑**" → 填入上游 API Key → 保存

UI 里 Key 默认显示为 `sk-XX***XXXX` 格式, 必须点 "查看完整 API Key" 才会显示完整内容, 防止被截图泄露。

**手工方式 (备选)**: 通过 psql 写入

```bash
docker exec -it cloud-server-postgres-1 psql -U inkuo -d inkuo_cloud
```

```sql
-- DeepSeek
UPDATE "ModelConfigs"
SET "UpstreamApiKey" = 'sk-your-deepseek-key-here'
WHERE "ModelName" = 'deepseek-chat';

-- OpenAI
UPDATE "ModelConfigs"
SET "UpstreamApiKey" = 'sk-your-openai-key-here'
WHERE "ModelName" = 'gpt-4o-mini';

-- 调整价格 (元 / 1M tokens, input + output)
UPDATE "ModelConfigs"
SET "InputPricePerMTokens" = 0.8, "OutputPricePerMTokens" = 2.0
WHERE "ModelName" = 'deepseek-chat';
```

> V1 把 key 明文存数据库。生产化请加 DataProtection 加密 (后续迭代)。

### 4. 创建邀请码 & 兑换码

通过 Billing 服务的 admin 端点:

```bash
# 邀请码: 用户注册时用的 (注册送免费额度)
curl -X POST http://localhost:8081/admin/invite-codes \
  -H "X-Admin-Token: $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"code":"BETA2026","freeQuotaCents":500,"maxUses":1000}'

# 兑换码: 已注册用户充值 / 开套餐
curl -X POST http://localhost:8081/admin/redemption-codes \
  -H "X-Admin-Token: $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"code":"PLUS-MAR2026","creditCents":2900,"maxUses":100}'

# 也可绑定到具体套餐:
curl -X POST http://localhost:8081/admin/redemption-codes \
  -H "X-Admin-Token: $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "code":"PRO-MAR2026",
    "creditCents":0,
    "planId":"00000000-0000-0000-0000-000000000003",
    "maxUses":100
  }'
```

套餐的 `Id` 列表:
- `00000000-0000-0000-0000-000000000001` - Free
- `00000000-0000-0000-0000-000000000002` - Plus
- `00000000-0000-0000-0000-000000000003` - Pro
- `00000000-0000-0000-0000-000000000004` - Max

### 5. 日常运维

```bash
# 看日志
docker compose --env-file .env logs -f api

# 数据库备份
docker exec cloud-server-postgres-1 pg_dump -U inkuo inkuo_cloud > backup-$(date +%Y%m%d).sql

# 停服
docker compose --env-file .env down

# 验证服务
bash cloud-server/scripts/smoke-test.sh
```

### 6. 数据迁移 (生产第一次部署)

生产环境部署第一次会跑 EF Core migration 创建所有表 + 灌入种子数据。后续更新需要:

```bash
cd cloud-server/src
ConnectionStrings__Postgres="<prod-conn>" \
  Jwt__Secret="<any>" \
  dotnet ef database update \
    --project Inkuso.Cloud.Core \
    --startup-project Inkuso.Cloud.Api
```

### 7. Admin Web UI (运营人员)

第一次部署完成后, 浏览器打开 `https://admin.inkuo.com`:

1. **首次登录**: 用 `.env` 里 `ADMIN_SEED_USERNAME` / `ADMIN_SEED_PASSWORD` (默认 `admin` / `admin123`) 登录
2. **改密码**: 右上角头像 → "修改密码" → 立即改
3. **配置模型**: "模型配置" 页面 → 填入上游 LLM 的 API Key
4. **创建邀请码**: "邀请码" → 新增 (例如 `BETA2026`, 1000 次, 每注册送 ¥5)
5. **生成兑换码**: "兑换码" → 新增 (例如 `PLUS-MAR2026`, 充值 ¥29 或开 Plus 套餐)
6. **看数据**: "仪表盘" 看用户增长、收入趋势; "用量记录" 看每条 chat 调用的明细
7. **加管理员**: "管理员" → 新增 (只 superadmin 能做, 给运营同事建 admin 账号, 不给 superadmin 权限)

#### 7.1 nginx 反向代理 admin 面板

```nginx
# /etc/nginx/sites-available/admin.inkuo.com
server {
    listen 443 ssl http2;
    server_name admin.inkuo.com;

    ssl_certificate /etc/letsencrypt/live/admin.inkuo.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/admin.inkuo.com/privkey.pem;

    # 限制只让公司出口 IP 访问 (强烈建议!)
    allow 203.0.113.0/24;       # 公司办公网
    allow 198.51.100.42/32;     # 跳板机
    deny all;

    location / {
        proxy_pass http://127.0.0.1:8082;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

#### 7.2 安全建议

- **强密码**: `ADMIN_SEED_PASSWORD` 用 `openssl rand -base64 24` 生成
- **IP 白名单**: 通过 nginx 或 Cloudflare Access 限制 admin 域名的访问源
- **二次验证**: V2 计划支持 TOTP, 当前只有密码
- **审计**: 所有 admin 操作 (调账、改套餐、删除用户) 会写日志到 console, V2 接入 Loki / Datadog
- **定期 rotate**: 每月换一次 superadmin 密码

---

## 二、桌面端打包 (Tauri)

桌面端代码嵌在新功能里, **老用户升级后 Settings 文件会无缝向后兼容** —— `cloud` 字段缺失时自动 fallback 到本地模式。

### 1. 本地 dev (开发用)

```bash
cd inkuo
pnpm install
pnpm tauri dev
```

打开设置面板, 模式切到 "inkuo Cloud", 填你的服务器地址 (例如 `https://cloud.inkuo.com`) 即可登录使用。

### 2. 桌面端 release build

#### macOS

```bash
pnpm tauri build --target universal-apple-darwin
# 产物:
#   src-tauri/target/release/bundle/macos/inkuo.app
#   src-tauri/target/release/bundle/dmg/inkuo_0.x.x_universal.dmg
```

#### Windows

```bash
pnpm tauri build
# 产物:
#   src-tauri/target/release/bundle/msi/inkuo_0.x.x_x64_en-US.msi
```

#### Linux (deb + AppImage)

```bash
pnpm tauri build
# 产物:
#   src-tauri/target/release/bundle/deb/inkuo_0.x.x_amd64.deb
#   src-tauri/target/release/bundle/appimage/inkuo_0.x.x_amd64.AppImage
```

### 3. 发布前检查清单

- [ ] 在生产服务器上确认 `docker compose ps` 三个服务都 Up
- [ ] Postgres 数据已写入上游 API keys
- [ ] 已创建一个邀请码给 Beta 测试用户
- [ ] smoke-test 通过: `bash cloud-server/scripts/smoke-test.sh`
- [ ] 桌面端 Settings 已经把 `cloud_mode_enabled = false` 作为默认值, 老用户不会看到 cloud tab 突然出现强扰
- [ ] Tauri 应用打包后能在 macOS / Windows / Linux 启动, 设置面板的 "inkuo Cloud" tab 可见且点击无错误

### 4. 升级迁移

桌面端 Settings 文件 schema 做了向后兼容:

- 老 users 的 Settings.json 没有 `cloud` 字段 → 自动 fallback 到本地模式, UI 仍可点 "inkuo Cloud" tab 进入登录
- 老 users 的 api_configs[] 完全保留, 切回本地 tab 一行不变
- token 明文存在前端 (V1); V2 计划移到 OS keychain (tauri-plugin-stronghold)

---

## 三、整体架构

```
用户桌面端 (Tauri)
    │
    │  HTTPS, JWT Bearer, SSE streaming
    ▼
   ┌─────────────────────────────────────┐
   │         Cloud Server                │
   │  ┌─────────┐      ┌──────────────┐  │
   │  │ Api:8080│◄────►│ Billing:8081 │  │
   │  └────┬────┘      └──────┬───────┘  │
   │       │                  │          │
   │       └──────┬───────────┘          │
   │              ▼                      │
   │      ┌──────────────┐               │
   │      │ PostgreSQL   │               │
   │      └──────────────┘               │
   └────────────────┬────────────────────┘
                    │
                    ▼
        ┌───────────────────────┐
        │ 上游 LLM APIs         │
        │ DeepSeek / OpenAI etc │
        └───────────────────────┘
```

---

## 四、常见问题

### Q: 老用户的本地 API 配置会不会被覆盖?
**不会。** `cloud_mode_enabled` 默认 `false`, 老的 `apiConfigs[]` 数组完全保留。点击 "inkuo Cloud" tab 也不会动它。只有当用户**主动**登录并点击 "切换到云端" 时, 新路由才生效。

### Q: token 会不会泄露?
V1 中 token 明文存在前端的 Settings 里 (`zustand/persist` 在浏览器 storage)。V2 计划用 tauri-plugin-stronghold 加密到 OS keychain。

### Q: 为什么账单统计目前显示 0?
因为没有真实上游调用。Billing worker 每 15 分钟会跑一次对账, 但 usage records 只有在 chat 端点真的发出请求 + 上游返回 `usage` 块后才写入。

### Q: 上游超时怎么办?
当前 LlmForwarder 用了 120s HTTP timeout + `HttpCompletionOption.ResponseHeadersRead` 做真正的 streaming。可以根据上游 P99 调小。