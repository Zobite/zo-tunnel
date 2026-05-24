#!/bin/bash
set -e

# E2E test for Zo Tunnel — tests HTTP mode + TCP mode
# Tests: local HTTP server → zo-tunnel-client → zo-tunnel-server → curl

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

SERVER_BIN="$PROJECT_DIR/target/release/zo-tunnel-server"
CLIENT_BIN="$PROJECT_DIR/target/release/zo-tunnel-client"

CONTROL_PORT=16200
PUBLIC_PORT=16210
DASHBOARD_PORT=16220
LOCAL_PORT=13000
LOCAL_PORT2=13001
TOKEN="test_secret_42"

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

# 2. Start Zo Tunnel server
echo "2️⃣  Starting zo-tunnel-server..."
RUST_LOG=info $SERVER_BIN \
    --control-port $CONTROL_PORT \
    --public-port $PUBLIC_PORT \
    --dashboard-port $DASHBOARD_PORT \
    --token "$TOKEN" 2>&1 | sed 's/^/   [server] /' &
SERVER_PID=$!
sleep 2

echo "   ✅ Server started"

# 3. Start HTTP mode client
echo "3️⃣  Starting zo-tunnel-client (HTTP mode, id=test-app)..."
RUST_LOG=info $CLIENT_BIN \
    --server 127.0.0.1:$CONTROL_PORT \
    --local 127.0.0.1:$LOCAL_PORT \
    --id test-app \
    --token "$TOKEN" 2>&1 | sed 's/^/   [http-client] /' &
CLIENT_PID=$!
sleep 2

echo "   ✅ HTTP client started"

# 4. Start TCP mode client
echo "4️⃣  Starting zo-tunnel-client (TCP mode, id=tcp-app)..."
RUST_LOG=info $CLIENT_BIN \
    --server 127.0.0.1:$CONTROL_PORT \
    --local 127.0.0.1:$LOCAL_PORT2 \
    --id tcp-app \
    --token "$TOKEN" \
    --tcp 2>&1 | sed 's/^/   [tcp-client] /' &
CLIENT2_PID=$!
sleep 2

echo "   ✅ TCP client started"

# 5. Test HTTP tunnel (path-based routing: /test-app/)
echo ""
echo "5️⃣  Testing HTTP tunnel: curl http://127.0.0.1:$PUBLIC_PORT/test-app/"
echo "────────────────────────────────────────"
RESPONSE=$(curl -s --max-time 10 http://127.0.0.1:$PUBLIC_PORT/test-app/ 2>&1 || echo "CURL_FAILED")

if echo "$RESPONSE" | grep -qi "PLAN.md\|Cargo.toml\|Directory listing\|<!DOCTYPE"; then
    echo "   ✅ HTTP TUNNEL WORKS! Got response through tunnel"
    echo ""
    echo "   Response preview:"
    echo "$RESPONSE" | head -3 | sed 's/^/   │ /'
else
    echo "   ❌ HTTP TUNNEL FAILED"
    echo "   Response: $RESPONSE"
fi

# 6. Find allocated TCP port from dashboard
echo ""
echo "6️⃣  Finding allocated TCP port for tcp-app..."
CLIENTS_RESPONSE=$(curl -s --max-time 5 http://127.0.0.1:$DASHBOARD_PORT/api/clients 2>&1 || echo "CURL_FAILED")
TCP_PORT=$(echo "$CLIENTS_RESPONSE" | python3 -c "import sys,json;clients=json.load(sys.stdin);port=[c['tcp_port'] for c in clients if c['client_id']=='tcp-app'][0];print(port)" 2>/dev/null || echo "")

if [ -n "$TCP_PORT" ] && [ "$TCP_PORT" != "None" ] && [ "$TCP_PORT" != "null" ]; then
    echo "   ✅ TCP port allocated: $TCP_PORT"

    # 7. Test TCP tunnel — connect to the dedicated port
    echo ""
    echo "7️⃣  Testing TCP tunnel: curl http://127.0.0.1:$TCP_PORT/"
    echo "────────────────────────────────────────"
    TCP_RESPONSE=$(curl -s --max-time 10 http://127.0.0.1:$TCP_PORT/ 2>&1 || echo "CURL_FAILED")

    if echo "$TCP_RESPONSE" | grep -qi "e2e_test\|build.sh\|install.sh\|Directory listing\|<!DOCTYPE"; then
        echo "   ✅ TCP TUNNEL WORKS! Got response through raw TCP tunnel"
        echo ""
        echo "   Response preview:"
        echo "$TCP_RESPONSE" | head -3 | sed 's/^/   │ /'
    else
        echo "   ❌ TCP TUNNEL FAILED"
        echo "   Response: $TCP_RESPONSE"
    fi
else
    echo "   ❌ Could not find TCP port for tcp-app"
    echo "   Clients: $CLIENTS_RESPONSE"
fi

# 8. Test dashboard API
echo ""
echo "8️⃣  Testing dashboard API..."
DASH_RESPONSE=$(curl -s --max-time 5 http://127.0.0.1:$DASHBOARD_PORT/api/status 2>&1 || echo "CURL_FAILED")

if echo "$DASH_RESPONSE" | grep -q "running"; then
    echo "   ✅ Dashboard: $DASH_RESPONSE"
else
    echo "   ❌ Dashboard failed: $DASH_RESPONSE"
fi

# 9. Verify both clients show in clients API
echo ""
echo "9️⃣  Verifying both clients..."
if echo "$CLIENTS_RESPONSE" | grep -q "test-app" && echo "$CLIENTS_RESPONSE" | grep -q "tcp-app"; then
    echo "   ✅ Both clients registered!"
    echo "   $CLIENTS_RESPONSE" | python3 -c "
import sys, json
clients = json.load(sys.stdin)
for c in clients:
    mode = f'TCP:{c[\"tcp_port\"]}' if c.get('tcp_port') else 'HTTP'
    print(f'   │ {c[\"client_id\"]:12} → {mode:10} | requests: {c[\"total_requests\"]}')
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
