# inkuo Cloud Server

C# ASP.NET Core minimal API server that powers **inkuo Cloud**: a managed, multi-tenant LLM gateway.

Desktop clients (the inkuo Tauri app) authenticate against this server with email + invite code, then forward all chat completions through it. The server handles:

- JWT-based auth (access token + refresh token)
- Multi-tier subscription plans (Free / Plus / Pro / Max)
- Per-token metering and quota enforcement
- Stripe-free billing via redemption codes (manual top-up flow for now)
- LLM upstream forwarding with SSE streaming to the desktop client
- **A full React + Ant Design admin web UI** for operators: dashboard, users, plans, models, invite codes, redemption codes, usage analytics, admin user management

## Architecture

```
┌─────────────────┐   ┌──────────────────────────────────────────┐
│  inkuo Desktop  │   │         inkuo Cloud Server               │
│  (Tauri/Rust)   │──▶│  ┌─────────────┐    ┌─────────────────┐  │
│                 │   │  │  Api svc    │───▶│ Billing svc     │  │
│  local mode:    │   │  │  (port 8080)│    │ (port 8081)     │  │
│  direct LLM     │   │  └──────┬──────┘    └────────┬────────┘  │
│  cloud mode:    │   │         │                    │           │
│  via this ──────┼───┼─────────┼────────────────────┼───────────│
│                 │   │   ┌─────▼────────────────────▼────────┐  │
│  admin user ────┼──▶│   │  Admin svc (port 8082)            │  │
│  via web UI     │   │   │  - React SPA at /                  │  │
└─────────────────┘   │   │  - /api/* REST endpoints           │  │
                      │   └──────────────┬────────────────────┘  │
                      │                  ▼                       │
                      │  ┌──────────────────────────────────┐    │
                      │  │     PostgreSQL (shared)          │    │
                      │  └──────────────────────────────────┘    │
                      └──────────────────────────────────────────┘
                                       │
                                       ▼
                            ┌──────────────────────┐
                            │  Upstream LLM APIs   │
                            │  (DeepSeek / OpenAI) │
                            └──────────────────────┘
```

## Quick start (local dev)

```bash
cp .env.example .env
# Edit .env: set JWT_SECRET and POSTGRES_PASSWORD

docker compose up -d --build

# Wait for migrations (~10s), then verify:
curl http://localhost:8080/health    # API
curl http://localhost:8081/health    # Billing
curl http://localhost:8082/health    # Admin

# Register a test user via API
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"inviteCode":"INKUO2026","email":"test@example.com","password":"testpass123"}'

# Or open the admin web UI
open http://localhost:8082
# Default login: admin / admin123 (CHANGE IMMEDIATELY)
```

The default invite code `INKUO2026` is seeded with 9999 uses and ¥5 free credit per registration.

## Admin Web UI

The admin panel at **http://localhost:8082** (production: behind your HTTPS reverse proxy) gives you:

- **Dashboard**: total users, today/month new users, active subscriptions, monthly revenue, total tokens used, invite/redemption code usage rates, plus ECharts visualisations:
  - 30-day usage trend (revenue, tokens, new users)
  - Plan distribution pie chart
  - Top-N model usage horizontal bar chart
- **Users**: paginated list with search/sort; view detail drawer (subscriptions + last 100 usage records + active refresh tokens); manually adjust balance with audit reason; revoke all sessions; delete user
- **Plans**: full CRUD (name, monthly fee, token limit, overage prices, enabled)
- **Models**: full CRUD (upstream provider/base URL/API key [masked by default, click to reveal]/display name/description/per-model pricing/sort order/enabled)
- **Invite codes**: full CRUD + enable/disable toggle
- **Redemption codes**: full CRUD + enable/disable toggle + bind-to-plan
- **Usage**: time-range filtered log of every chat-completion call (user, model, tokens, cost)
- **Admins**: superadmin can create/list/delete other admin users

All forms have validation; API keys are typed as `<first4>***<last4>` unless you click "查看完整 API Key".

### Default credentials

The first time the admin service starts with an empty database, it seeds a single `superadmin` user from `ADMIN_SEED_USERNAME` / `ADMIN_SEED_PASSWORD` env vars (default `admin` / `admin123`). **Change the password from the user menu in the top right immediately after first login.**

## Configuration

| Variable | Description | Default |
|---|---|---|
| `POSTGRES_PASSWORD` | PostgreSQL root password | `inkuo_dev_password` |
| `JWT_SECRET` | HMAC-SHA256 signing secret (≥32 chars) | (required) |
| `ADMIN_TOKEN` | Bearer for Billing service `/admin/*` legacy endpoints | (required) |
| `ADMIN_SEED_USERNAME` | Username of the bootstrap admin user (created on first run if no admin exists) | `admin` |
| `ADMIN_SEED_PASSWORD` | Password of the bootstrap admin user | `admin123` |

## Endpoints

### Auth (no auth required) — port 8080
- `POST /auth/register` — body: `{inviteCode, email, password}` → returns tokens + user
- `POST /auth/login` — body: `{email, password}` → returns tokens + user
- `POST /auth/refresh` — body: `{refreshToken}` → returns new access token

### Models (Bearer required) — port 8080
- `GET /v1/models` — list enabled upstream models (id, display_name, pricing)

### Chat (Bearer required) — port 8080
- `POST /v1/chat/completions` — OpenAI-compatible body, SSE stream returned
  - `model` field accepts either `model_config_id` (Guid) or upstream `model_name`

### Account (Bearer required) — port 8080
- `GET /account/me` — user info, balance, plan, monthly quota usage
- `GET /account/usage` — last 50 usage records

### Billing (Bearer required) — port 8080
- `POST /redeem` — body: `{code}` → activates subscription or adds credit

### Legacy Billing admin (X-Admin-Token required) — port 8081
- `POST /admin/redemption-codes` — create redemption codes
- `POST /admin/invite-codes` — create invite codes
- `GET /admin/stats` — aggregate stats (user count, revenue, etc.)

### Admin Web UI & API — port 8082
- `GET /` — React SPA (single-page app, all routes fall back to index.html)
- `POST /api/auth/login` — admin login → admin JWT (separate audience from customer JWT)
- All other `/api/*` endpoints require `Authorization: Bearer <admin_jwt>`. See `Inkuso.Cloud.Admin/Endpoints/` for the full list:
  - `/api/dashboard/{summary,usage-trend,plan-distribution,model-usage}`
  - `/api/users/...` (list, detail, adjust-balance, revoke-sessions, delete)
  - `/api/plans/...` (CRUD)
  - `/api/model-configs/...` (CRUD; pass `?includeKey=true` to see real API keys)
  - `/api/invite-codes/...` (CRUD + toggle)
  - `/api/redemption-codes/...` (CRUD + toggle)
  - `/api/usage/` (filterable by user/model/date range)
  - `/api/auth/{me,change-password,create}` (admin user management)

The admin JWT uses a separate audience (`inkuo-admin` by default, configurable via `Jwt__AdminAudience`) so it can never be used to call the customer-facing API.

## Database schema

EF Core migrations are applied automatically on startup (`db.Database.Migrate()`).

Tables: `users`, `plans`, `subscriptions`, `invite_codes`, `redemption_codes`, `model_configs`, `usage_records`, `refresh_tokens`, `admin_users`.

Seeded data on first boot:
- 4 plans: Free / Plus / Pro / Max
- 1 invite code: `INKUO2026` (¥5 free credit, 9999 uses)
- 3 model configs: DeepSeek-V3, GPT-4o Mini, GPT-4o (with placeholder upstream keys — set real ones via the admin UI)
- 1 admin user: `admin` / `admin123` (only if no admin exists yet)

## Upstream API keys

The seeded `model_configs` rows have empty `upstream_api_key`. Set them via the **Models** page in the admin UI (`http://localhost:8082/models` → click "查看完整 API Key" → edit). API keys are masked in the UI by default.

## Project layout

```
cloud-server/
├── docker-compose.yml
├── Dockerfile                 # Multi-stage: Node SPA + .NET services
├── .env.example
├── .dockerignore
├── README.md
├── DEPLOYMENT.md
├── scripts/
│   └── smoke-test.sh          # End-to-end test of all 3 services
├── admin-frontend/            # React + AntD admin SPA (Vite)
│   ├── package.json
│   ├── vite.config.ts
│   └── src/
│       ├── api/               # Typed axios client for /api/*
│       ├── layouts/           # AdminLayout (sidebar + header)
│       └── pages/             # Login + 8 admin pages
└── src/
    ├── Inkuso.Cloud.slnx
    ├── Inkuso.Cloud.Core/     # EF Core entities, JWT, LLM forwarder
    ├── Inkuso.Cloud.Api/      # ASP.NET Core Minimal API (port 8080)
    ├── Inkuso.Cloud.Billing/  # Reconciliation worker + legacy admin endpoints (port 8081)
    └── Inkuso.Cloud.Admin/    # Admin API + SPA host (port 8082)
```

## Production checklist

- [ ] Set `JWT_SECRET` to a real 48+ char random value
- [ ] Set `POSTGRES_PASSWORD` to a strong password
- [ ] Set `ADMIN_TOKEN` to a strong random value
- [ ] Set `ADMIN_SEED_PASSWORD` to a strong bootstrap password (and change it immediately after first login)
- [ ] Put behind nginx/Caddy with HTTPS
- [ ] Set up PostgreSQL backups (e.g. `pg_dump` cron)
- [ ] Encrypt `upstream_api_key` column (currently plaintext)
- [ ] Add rate limiting middleware
- [ ] Wire logs to a central aggregator
- [ ] Rotate the bootstrap admin password via the UI's "修改密码" menu on first deploy