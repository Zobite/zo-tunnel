// Zo Tunnel Dashboard — auto-refreshing client with authentication

const REFRESH_MS = 2000;
let refreshInterval = null;

// ─── Utilities ──────────────────────────────────────────────────

function formatBytes(bytes) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function formatDuration(secs) {
    if (secs < 60) return secs + 's';
    if (secs < 3600) return Math.floor(secs / 60) + 'm ' + (secs % 60) + 's';
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return h + 'h ' + m + 'm';
}

function formatNumber(n) {
    return n.toLocaleString();
}

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

async function fetchJSON(url) {
    const resp = await fetch(url);
    if (resp.status === 401) {
        // Session expired or invalid
        showLogin();
        throw new Error('Unauthorized');
    }
    if (!resp.ok) throw new Error(resp.statusText);
    return resp.json();
}

// ─── Auth Flow ──────────────────────────────────────────────────

function showLogin() {
    document.getElementById('login-screen').style.display = 'flex';
    document.getElementById('dashboard').style.display = 'none';
    if (refreshInterval) {
        clearInterval(refreshInterval);
        refreshInterval = null;
    }
}

function showDashboard() {
    document.getElementById('login-screen').style.display = 'none';
    document.getElementById('dashboard').style.display = 'block';
    // Start auto-refresh
    refresh();
    if (refreshInterval) clearInterval(refreshInterval);
    refreshInterval = setInterval(refresh, REFRESH_MS);
}

async function checkAuth() {
    try {
        const resp = await fetch('/api/auth/check');
        const data = await resp.json();

        // Handle TLS warning
        const tlsWarning = document.getElementById('tls-warning');
        if (data.tls_enabled) {
            tlsWarning.style.display = 'none';
        } else {
            tlsWarning.style.display = 'block';
        }

        if (!data.auth_required || data.authenticated) {
            showDashboard();
        } else {
            showLogin();
        }
    } catch (e) {
        // Can't reach server — show login anyway
        showLogin();
    }
}

async function handleLogin(event) {
    event.preventDefault();

    const tokenInput = document.getElementById('admin-token');
    const errorEl = document.getElementById('login-error');
    const btnEl = document.getElementById('login-btn');

    const token = tokenInput.value.trim();
    if (!token) {
        errorEl.textContent = 'Please enter your admin token';
        return;
    }

    btnEl.disabled = true;
    btnEl.textContent = 'Signing in...';
    errorEl.textContent = '';

    try {
        const resp = await fetch('/api/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ token: token }),
        });

        const data = await resp.json();

        if (data.success) {
            tokenInput.value = '';
            showDashboard();
        } else {
            errorEl.textContent = data.message || 'Login failed';
        }
    } catch (e) {
        errorEl.textContent = 'Connection error. Please try again.';
    } finally {
        btnEl.disabled = false;
        btnEl.textContent = 'Sign In';
    }
}

async function handleLogout() {
    try {
        await fetch('/api/logout', { method: 'POST' });
    } catch (e) {
        // Ignore errors on logout
    }
    showLogin();
}

// ─── Dashboard Refresh ──────────────────────────────────────────

async function refresh() {
    try {
        const [status, clients, metrics] = await Promise.all([
            fetchJSON('/api/status'),
            fetchJSON('/api/clients'),
            fetchJSON('/api/metrics'),
        ]);

        // Status badge
        const badge = document.getElementById('status-badge');
        badge.textContent = 'online';
        badge.className = 'badge online';

        // Uptime
        document.getElementById('uptime').textContent =
            'Uptime: ' + formatDuration(metrics.uptime_secs);

        // Stats
        document.getElementById('stat-clients').textContent = status.connected_clients;
        document.getElementById('stat-requests').textContent = formatNumber(metrics.total_requests);
        document.getElementById('stat-active').textContent = metrics.active_connections;
        document.getElementById('stat-data').textContent =
            formatBytes(metrics.total_bytes_in + metrics.total_bytes_out);
        document.getElementById('stat-failed-auth').textContent = metrics.failed_auth;
        document.getElementById('stat-rate-limited').textContent = metrics.rate_limited;

        // Clients table — using safe DOM methods
        const tbody = document.getElementById('clients-body');
        tbody.replaceChildren(); // clear safely

        if (clients.length === 0) {
            const tr = document.createElement('tr');
            const td = document.createElement('td');
            td.colSpan = 7;
            td.className = 'empty';
            td.textContent = 'No clients connected';
            tr.appendChild(td);
            tbody.appendChild(tr);
        } else {
            clients.forEach(function (c) {
                const tr = document.createElement('tr');

                // Client ID
                const tdId = document.createElement('td');
                tdId.className = 'client-id';
                tdId.textContent = c.client_id;
                tr.appendChild(tdId);

                // Mode
                const tdMode = document.createElement('td');
                const modeSpan = document.createElement('span');
                if (c.tcp_port) {
                    modeSpan.style.color = 'var(--orange)';
                    modeSpan.textContent = 'TCP:' + c.tcp_port;
                } else {
                    modeSpan.style.color = 'var(--green)';
                    modeSpan.textContent = 'HTTP';
                }
                tdMode.appendChild(modeSpan);
                tr.appendChild(tdMode);

                // Connected
                const tdConn = document.createElement('td');
                tdConn.textContent = formatDuration(c.connected_at_secs) + ' ago';
                tr.appendChild(tdConn);

                // Requests
                const tdReq = document.createElement('td');
                tdReq.textContent = formatNumber(c.total_requests);
                tr.appendChild(tdReq);

                // Active
                const tdActive = document.createElement('td');
                tdActive.textContent = c.active_streams;
                tr.appendChild(tdActive);

                // Data In
                const tdIn = document.createElement('td');
                tdIn.textContent = formatBytes(c.bytes_in);
                tr.appendChild(tdIn);

                // Data Out
                const tdOut = document.createElement('td');
                tdOut.textContent = formatBytes(c.bytes_out);
                tr.appendChild(tdOut);

                tbody.appendChild(tr);
            });
        }

    } catch (e) {
        if (e.message === 'Unauthorized') return; // Already handled
        const badge = document.getElementById('status-badge');
        badge.textContent = 'offline';
        badge.className = 'badge offline';
    }
}

// ─── Init ───────────────────────────────────────────────────────

document.getElementById('login-form').addEventListener('submit', handleLogin);
document.getElementById('logout-btn').addEventListener('click', handleLogout);

// Check auth on page load
checkAuth();
