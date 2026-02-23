#!/usr/bin/env bash
# E2E verification: panel is running; login, create user, get links.
# Usage: BASE_URL=http://localhost:8080 ADMIN_USER=admin ADMIN_PASS=yourpass ./temp/e2e_panel_verify.sh

set -e
BASE_URL="${BASE_URL:-http://localhost:8080}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-}"
if [[ -z "$ADMIN_PASS" ]]; then
  echo "Set ADMIN_PASS (and optionally BASE_URL, ADMIN_USER)" >&2
  exit 1
fi

echo "1. Health..."
curl -sf "${BASE_URL}/health" | grep -q ok || { echo "Health failed"; exit 1; }
echo "   OK"

echo "2. Login..."
TOKEN=$(curl -sf -X POST "${BASE_URL}/api/admin/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "username=${ADMIN_USER}&password=${ADMIN_PASS}" | sed -n 's/.*"access_token":"\([^"]*\)".*/\1/p')
[[ -n "$TOKEN" ]] || { echo "Login failed"; exit 1; }
echo "   OK"

echo "3. Create user..."
CREATE_RESP=$(curl -sf -X POST "${BASE_URL}/api/users" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"username":"e2euser","data_limit":null,"note":"E2E test"}')
echo "$CREATE_RESP" | grep -q '"username":"e2euser"' || { echo "Create user failed: $CREATE_RESP"; exit 1; }
echo "   OK"

echo "4. Get links..."
LINKS=$(curl -sf "${BASE_URL}/api/users/e2euser/links" -H "Authorization: Bearer ${TOKEN}")
echo "$LINKS" | grep -q '"tg_link"' || { echo "Get links failed: $LINKS"; exit 1; }
echo "   OK"
echo "   tg_link: $(echo "$LINKS" | sed -n 's/.*"tg_link":"\([^"]*\)".*/\1/p')"

echo ""
echo "E2E verification passed."
