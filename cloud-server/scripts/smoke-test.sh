#!/usr/bin/env bash
# End-to-end smoke test for the inkuo Cloud Server stack (API + Billing + Admin).
# Run after `docker compose up -d --build`. Export the same explicit
# bootstrap credentials used by the Admin container; this script has no
# built-in production or test password.
set -euo pipefail

BASE=${BASE:-http://localhost:8080}
BILLING=${BILLING:-http://localhost:8081}
ADMIN_PANEL=${ADMIN_PANEL:-http://localhost:8082}
ADMIN_USER=${ADMIN_SEED_USERNAME:?export ADMIN_SEED_USERNAME before running the smoke test}
ADMIN_PASS=${ADMIN_SEED_PASSWORD:?export ADMIN_SEED_PASSWORD before running the smoke test}

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
ADMIN_LOGIN=$(curl -fsS -X POST "$ADMIN_PANEL/api/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"$ADMIN_USER\",\"password\":\"$ADMIN_PASS\"}")
ADMIN_JWT=$(echo "$ADMIN_LOGIN" | python3 -c "import sys, json; print(json.load(sys.stdin)['access_token'])")
[ -n "$ADMIN_JWT" ] && green "  ✓ admin JWT obtained"

echo "==> 3. Admin /me"
curl -fsS "$ADMIN_PANEL/api/auth/me" -H "Authorization: Bearer $ADMIN_JWT" | python3 -m json.tool

echo "==> 4. Admin dashboard summary"
curl -fsS "$ADMIN_PANEL/api/dashboard/summary" -H "Authorization: Bearer $ADMIN_JWT" | python3 -m json.tool

echo "==> 5. Admin plans list"
curl -fsS "$ADMIN_PANEL/api/plans/" -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', len(d), 'plans:', ', '.join(p['name'] for p in d))"

echo "==> 6. Admin model-configs list (key masked)"
curl -fsS "$ADMIN_PANEL/api/model-configs/" -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', len(d), 'models'); [print('   -', m['display_name'], '|', m['upstream_provider'], '| key=', m['upstream_api_key_masked'] or '(empty)') for m in d]"

echo "==> 7. Admin: create one-use invite code"
INVITE_CODE="SMOKE-INVITE-$(date +%s)-$RANDOM"
curl -fsS -X POST "$ADMIN_PANEL/api/invite-codes/" \
  -H "Authorization: Bearer $ADMIN_JWT" \
  -H "Content-Type: application/json" \
  -d "{\"code\":\"$INVITE_CODE\",\"free_points\":1000,\"max_uses\":1,\"expires_at\":null,\"enabled\":true}" | python3 -m json.tool

echo "==> 8. Admin: create redemption code"
CODE=$(curl -fsS -X POST "$ADMIN_PANEL/api/redemption-codes/" \
  -H "Authorization: Bearer $ADMIN_JWT" \
  -H "Content-Type: application/json" \
  -d "{\"code\":\"SMOKE-REDEEM-$(date +%s)-$RANDOM\",\"credit_points\":5000,\"plan_id\":null,\"max_uses\":1,\"expires_at\":null,\"enabled\":true}" | python3 -c "import sys, json; print(json.load(sys.stdin)['code'])")
green "  created code: $CODE"

echo "==> 9. Customer registers via the one-use smoke invite"
EMAIL="smoke-$(date +%s)@example.com"
RESP=$(curl -fsS -X POST "$BASE/auth/register" \
  -H "Content-Type: application/json" \
  -d "{\"invite_code\":\"$INVITE_CODE\",\"email\":\"$EMAIL\",\"password\":\"smoke-pass-123\"}")
echo "$RESP" | python3 -c "import sys, json; d=json.load(sys.stdin); print('  user_id=', d['user']['id'], 'balance_points=', d['user']['balance_points'])"
TOKEN=$(echo "$RESP" | python3 -c "import sys, json; print(json.load(sys.stdin)['access_token'])")
RT=$(echo "$RESP" | python3 -c "import sys, json; print(json.load(sys.stdin)['refresh_token'])")

echo "==> 10. Customer login + refresh"
curl -fsS -X POST "$BASE/auth/login" \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"$EMAIL\",\"password\":\"smoke-pass-123\"}" >/dev/null && green "  ✓ login OK"
curl -fsS -X POST "$BASE/auth/refresh" \
  -H "Content-Type: application/json" \
  -d "{\"refresh_token\":\"$RT\"}" >/dev/null && green "  ✓ refresh OK"

echo "==> 11. Customer lists cloud models"
curl -fsS "$BASE/v1/models" -H "Authorization: Bearer $TOKEN" | \
  python3 -c "import sys, json; d=json.load(sys.stdin)['data']; print('  ', len(d), 'models:', ', '.join(m['display_name'] for m in d))"

echo "==> 12. Customer redeems admin-issued code"
curl -fsS -X POST "$BASE/redeem" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"code\":\"$CODE\"}" | python3 -m json.tool

echo "==> 13. Admin lists users (should now see the new customer)"
curl -fsS "$ADMIN_PANEL/api/users/?pageSize=5" -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', d['total'], 'users total'); [print('   -', u['email'], '| balance ¥', u['balance_points']/1000, '| plan:', u['plan_name']) for u in d['items']]"

echo "==> 14. Admin adjusts user balance"
USERID=$(echo "$RESP" | python3 -c "import sys, json; print(json.load(sys.stdin)['user']['id'])")
curl -fsS -X POST "$ADMIN_PANEL/api/users/$USERID/adjust-balance" \
  -H "Authorization: Bearer $ADMIN_JWT" \
  -H "Content-Type: application/json" \
  -d '{"delta_points":2000,"reason":"smoke test bonus"}' | python3 -m json.tool

echo "==> 15. Admin usage dashboard trend (last 30 days)"
curl -fsS "$ADMIN_PANEL/api/dashboard/usage-trend" -H "Authorization: Bearer $ADMIN_JWT" | \
  python3 -c "import sys, json; d=json.load(sys.stdin); print('  ', len(d), 'data points, sample first:', d[0])"

echo "==> 16. Admin SPA fallback (any unknown route returns index.html)"
check "/users route" curl -fsS "$ADMIN_PANEL/users" | grep -q "inkuo Cloud Admin"
check "/plans route" curl -fsS "$ADMIN_PANEL/plans" | grep -q "inkuo Cloud Admin"

echo
green "==> All smoke tests passed ✓"
green "    Browse the admin panel at $ADMIN_PANEL"
green "    Signed in with the explicit bootstrap admin credentials from the environment"
