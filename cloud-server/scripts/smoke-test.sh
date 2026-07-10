#!/usr/bin/env bash
# End-to-end smoke test for the inkuo Cloud Server stack (API + Billing + Admin).
# Run after `docker compose up -d --build` and assuming:
#   POSTGRES_PASSWORD=inkuo_dev_pwd (or whatever you set in .env)
#   JWT_SECRET=test-jwt-secret-32-chars-or-longer-for-dev-only
#   ADMIN_TOKEN=test-admin-token
#   ADMIN_SEED_PASSWORD=admin123
set -euo pipefail

BASE=http://localhost:8080
BILLING=http://localhost:8081
ADMIN_PANEL=http://localhost:8082
ADMIN_TOKEN=test-admin-token
ADMIN_USER=admin
ADMIN_PASS=admin123

red()    { printf '\033[31m%s\033[0m\n' "$*" >&2; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }

check() {
    local label=$1; shift
    if "$@"; then
        green "  ✓ $label"
    else
        red "  ✗ $label"
        exit 1
    fi
}

echo "==> 1. Health checks"
check "api health"      curl -fsS "$BASE/health" >/dev/null
check "billing health"  curl -fsS "$BILLING/health" >/dev/null
check "admin health"    curl -fsS "$ADMIN_PANEL/health" >/dev/null
check "admin SPA index" curl -fsS "$ADMIN_PANEL/" >/dev/null

echo "==> 2. Admin login"
ADMIN_LOGIN=$(curl -sS -X POST $ADMIN_PANEL/api/auth/login \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASS\"}")
ADMIN_JWT=$(echo "$ADMIN_LOGIN" | python3 -c "import sys, json; print(json.load(sys.stdin)['accessToken'])")
[ -n "$ADMIN_JWT" ] && green "  ✓ admin JWT obtained"

echo "==> 3. Admin /me"
curl -sS $ADMIN_PANEL/api/auth/me -H "Authorization: Bearer $ADMIN_JWT" | python3 -m json.tool

echo "==> 4. Admin dashboard summary"
curl -sS $ADMIN_PANEL/api/dashboard/summary -H "Authorization: Bearer $ADMIN_JWT" | python3 -m json.tool

echo "==> 5. Admin plans list"
curl -sS $ADMIN_PANEL/api/plans/ -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', len(d), 'plans:', ', '.join(p['name'] for p in d))"

echo "==> 6. Admin model-configs list (key masked)"
curl -sS $ADMIN_PANEL/api/model-configs/ -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', len(d), 'models'); [print('   -', m['displayName'], '|', m['upstreamProvider'], '| key=', m['upstreamApiKeyMasked'] or '(empty)') for m in d]"

echo "==> 7. Admin: create invite code"
curl -sS -X POST $ADMIN_PANEL/api/invite-codes/ \
  -H "Authorization: Bearer $ADMIN_JWT" \
  -H "Content-Type: application/json" \
  -d "{\"code\":\"SMOKE-$(date +%s)\",\"freeQuotaCents\":100,\"maxUses\":5,\"enabled\":true}" | python3 -m json.tool

echo "==> 8. Admin: create redemption code"
CODE=$(curl -sS -X POST $ADMIN_PANEL/api/redemption-codes/ \
  -H "Authorization: Bearer $ADMIN_JWT" \
  -H "Content-Type: application/json" \
  -d "{\"code\":\"SMOKE-$(date +%s)\",\"creditCents\":500,\"maxUses\":1,\"enabled\":true}" | python3 -c "import sys, json; print(json.load(sys.stdin)['code'])")
green "  created code: $CODE"

echo "==> 9. Customer register via INKUO2026 invite"
EMAIL="smoke-$(date +%s)@example.com"
RESP=$(curl -sS -X POST $BASE/auth/register \
  -H "Content-Type: application/json" \
  -d "{\"inviteCode\":\"INKUO2026\",\"email\":\"$EMAIL\",\"password\":\"smoke-pass-123\"}")
echo "$RESP" | python3 -c "import sys, json; d=json.load(sys.stdin); print('  user_id=', d['user']['id'], 'balance=', d['user']['balanceCents'])"
TOKEN=$(echo "$RESP" | python3 -c "import sys, json; print(json.load(sys.stdin)['accessToken'])")
RT=$(echo "$RESP" | python3 -c "import sys, json; print(json.load(sys.stdin)['refreshToken'])")

echo "==> 10. Customer login + refresh"
curl -sS -X POST $BASE/auth/login \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"smoke-pass-123\"}" >/dev/null && green "  ✓ login OK"
curl -sS -X POST $BASE/auth/refresh \
  -H "Content-Type: application/json" \
  -d "{\"refreshToken\":\"$RT\"}" >/dev/null && green "  ✓ refresh OK"

echo "==> 11. Customer lists cloud models"
curl -sS $BASE/v1/models -H "Authorization: Bearer $TOKEN" | \
  python3 -c "import sys, json; d=json.load(sys.stdin)['data']; print('  ', len(d), 'models:', ', '.join(m['displayName'] for m in d))"

echo "==> 12. Customer redeems admin-issued code"
curl -sS -X POST $BASE/redeem \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"code\":\"$CODE\"}" | python3 -m json.tool

echo "==> 13. Admin lists users (should now see the new customer)"
curl -sS "$ADMIN_PANEL/api/users/?pageSize=5" -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', d['total'], 'users total'); [print('   -', u['email'], '| balance ¥', u['balanceCents']/100, '| plan:', u['planName']) for u in d['items']]"

echo "==> 14. Admin adjusts user balance"
USERID=$(echo "$RESP" | python3 -c "import sys, json; print(json.load(sys.stdin)['user']['id'])")
curl -sS -X POST "$ADMIN_PANEL/api/users/$USERID/adjust-balance" \
  -H "Authorization: Bearer $ADMIN_JWT" \
  -H "Content-Type: application/json" \
  -d '{"deltaCents":200,"reason":"smoke test bonus"}' | python3 -m json.tool

echo "==> 15. Admin usage dashboard trend (last 30 days)"
curl -sS "$ADMIN_PANEL/api/dashboard/usage-trend" -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', len(d), 'data points, sample first:', d[0])"

echo "==> 16. Admin SPA fallback (any unknown route returns index.html)"
check "/users route" curl -fsS "$ADMIN_PANEL/users" | grep -q "inkuo Cloud Admin"
check "/plans route" curl -fsS "$ADMIN_PANEL/plans" | grep -q "inkuo Cloud Admin"

echo
green "==> All smoke tests passed ✓"
green "    Browse the admin panel at $ADMIN_PANEL"
green "    Default credentials: admin / admin123"