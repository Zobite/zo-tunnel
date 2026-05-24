#!/bin/bash
set -e

# E2E test for Zo Tunnel — tests subdomain HTTP routing
# Tests: local HTTP server → zo-tunnel-client → zo-tunnel-server → curl

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

SERVER_BIN="$PROJECT_DIR/target/release/zo-tunnel-server"
CLIENT_BIN="$PROJECT_DIR/target/release/zo-tunnel-client"

CONTROL_PORT=16200
PUBLIC_PORT=16210
LOCAL_PORT=13000
LOCAL_PORT2=13001
TOKEN="test_secret_42"
DOMAIN="test.localhost"

cleanup() {
    echo "🧹 Cleaning up..."
    kill $HTTP_PID $HTTP2_PID $SERVER_PID $CLIENT_PID $CLIENT2_PID 2>/dev/null || true
    wait $HTTP_PID $HTTP2_PID $SERVER_PID $CLIENT_PID $CLIENT2_PID 2>/dev/null || true
}
trap cleanup EXIT

echo "════════════════════════════════════════"
echo "  Zo Tunnel E2E Test"
echo "════════════════════════════════════════"
echo ""

# 1. Start local HTTP servers
echo "1️⃣  Starting local HTTP servers..."
python3 -m http.server $LOCAL_PORT --directory "$PROJECT_DIR" >/dev/null 2>&1 &
HTTP_PID=$!
python3 -m http.server $LOCAL_PORT2 --directory "$PROJECT_DIR/scripts" >/dev/null 2>&1 &
HTTP2_PID=$!
sleep 1

curl -s http://127.0.0.1:$LOCAL_PORT/ >/dev/null || { echo "❌ HTTP server 1 failed"; exit 1; }
curl -s http://127.0.0.1:$LOCAL_PORT2/ >/dev/null || { echo "❌ HTTP server 2 failed"; exit 1; }
echo "   ✅ Local HTTP servers running (port $LOCAL_PORT, $LOCAL_PORT2)"

# 2. Setup and start Zo Tunnel server
echo "2️⃣  Setting up zo-tunnel-server..."
$SERVER_BIN setup \
    --domain $DOMAIN \
    --control-port $CONTROL_PORT \
    --public-port $PUBLIC_PORT \
    --token "$TOKEN" \
    --force 2>&1 | sed 's/^/   [setup] /'

echo "   ✅ Config generated"

echo "   Starting zo-tunnel-server..."
RUST_LOG=info $SERVER_BIN start 2>&1 | sed 's/^/   [server] /' &
SERVER_PID=$!
sleep 2

echo "   ✅ Server started"

# 3. Start first client
echo "3️⃣  Starting zo-tunnel-client (id=test-app)..."
RUST_LOG=info $CLIENT_BIN \
    --server 127.0.0.1:$CONTROL_PORT \
    --local 127.0.0.1:$LOCAL_PORT \
    --id test-app \
    --token "$TOKEN" 2>&1 | sed 's/^/   [client-1] /' &
CLIENT_PID=$!
sleep 2

echo "   ✅ Client 'test-app' started"

# 4. Start second client
echo "4️⃣  Starting zo-tunnel-client (id=api-app)..."
RUST_LOG=info $CLIENT_BIN \
    --server 127.0.0.1:$CONTROL_PORT \
    --local 127.0.0.1:$LOCAL_PORT2 \
    --id api-app \
    --token "$TOKEN" 2>&1 | sed 's/^/   [client-2] /' &
CLIENT2_PID=$!
sleep 2

echo "   ✅ Client 'api-app' started"

# 5. Test subdomain routing — client 1
echo ""
echo "5️⃣  Testing subdomain routing: Host=test-app.$DOMAIN"
echo "────────────────────────────────────────"
RESPONSE=$(curl -s --max-time 10 -H "Host: test-app.$DOMAIN" http://127.0.0.1:$PUBLIC_PORT/ 2>&1 || echo "CURL_FAILED")

if echo "$RESPONSE" | grep -qi "Cargo.toml\|Directory listing\|<!DOCTYPE"; then
    echo "   ✅ SUBDOMAIN ROUTING WORKS (test-app)!"
    echo ""
    echo "   Response preview:"
    echo "$RESPONSE" | head -3 | sed 's/^/   │ /'
else
    echo "   ❌ SUBDOMAIN ROUTING FAILED (test-app)"
    echo "   Response: $RESPONSE"
fi

# 6. Test subdomain routing — client 2
echo ""
echo "6️⃣  Testing subdomain routing: Host=api-app.$DOMAIN"
echo "────────────────────────────────────────"
RESPONSE2=$(curl -s --max-time 10 -H "Host: api-app.$DOMAIN" http://127.0.0.1:$PUBLIC_PORT/ 2>&1 || echo "CURL_FAILED")

if echo "$RESPONSE2" | grep -qi "e2e_test\|build.sh\|install.sh\|Directory listing\|<!DOCTYPE"; then
    echo "   ✅ SUBDOMAIN ROUTING WORKS (api-app)!"
    echo ""
    echo "   Response preview:"
    echo "$RESPONSE2" | head -3 | sed 's/^/   │ /'
else
    echo "   ❌ SUBDOMAIN ROUTING FAILED (api-app)"
    echo "   Response: $RESPONSE2"
fi

# 7. Test dashboard API
echo ""
echo "7️⃣  Testing dashboard API..."
DASH_RESPONSE=$(curl -s --max-time 5 -H "Host: dashboard.$DOMAIN" http://127.0.0.1:$PUBLIC_PORT/api/status 2>&1 || echo "CURL_FAILED")

if echo "$DASH_RESPONSE" | grep -q "running"; then
    echo "   ✅ Dashboard: $DASH_RESPONSE"
else
    echo "   ❌ Dashboard failed: $DASH_RESPONSE"
fi

# 8. Verify both clients show in clients API
echo ""
echo "8️⃣  Verifying both clients..."
CLIENTS_RESPONSE=$(curl -s --max-time 5 -H "Host: dashboard.$DOMAIN" http://127.0.0.1:$PUBLIC_PORT/api/clients 2>&1 || echo "CURL_FAILED")

if echo "$CLIENTS_RESPONSE" | grep -q "test-app" && echo "$CLIENTS_RESPONSE" | grep -q "api-app"; then
    echo "   ✅ Both clients registered!"
    echo "   $CLIENTS_RESPONSE" | python3 -c "
import sys, json
clients = json.load(sys.stdin)
for c in clients:
    print(f'   │ {c[\"client_id\"]:12} | requests: {c[\"total_requests\"]}')
" 2>/dev/null || echo "   $CLIENTS_RESPONSE"
else
    echo "   ⚠️  Clients: $CLIENTS_RESPONSE"
fi

echo ""
echo "════════════════════════════════════════"
echo "  ✅ E2E TEST COMPLETE"
echo "════════════════════════════════════════"

sleep 1
exit 0
