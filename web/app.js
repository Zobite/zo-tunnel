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

        // Clients table — using safe DOM methods with reconciliation
        const tbody = document.getElementById('clients-body');

        if (clients.length === 0) {
            tbody.replaceChildren(); // clear safely
            const tr = document.createElement('tr');
            const td = document.createElement('td');
            td.colSpan = 7;
            td.className = 'empty';
            td.textContent = 'No clients connected';
            tr.appendChild(td);
            tbody.appendChild(tr);
        } else {
            // Remove the empty row if it exists
            const emptyRow = tbody.querySelector('tr td.empty');
            if (emptyRow) {
                tbody.replaceChildren();
            }

            // Gather existing rows
            const existingRows = {};
            const rowElements = tbody.querySelectorAll('tr');
            for (let i = 0; i < rowElements.length; i++) {
                const id = rowElements[i].dataset.id;
                if (id) {
                    existingRows[id] = rowElements[i];
                }
            }

            let lastRow = null;
            clients.forEach(function (c) {
                let tr = existingRows[c.client_id];
                if (tr) {
                    updateClientRow(tr, c);
                    delete existingRows[c.client_id];
                } else {
                    tr = buildClientRow(c);
                }

                // Preserve correct order
                const currentSibling = lastRow ? lastRow.nextSibling : tbody.firstChild;
                if (currentSibling !== tr) {
                    tbody.insertBefore(tr, currentSibling);
                }
                lastRow = tr;
            });

            // Remove rows that are no longer active
            for (const id in existingRows) {
                existingRows[id].remove();
            }
        }

    } catch (e) {
        if (e.message === 'Unauthorized') return; // Already handled
        const badge = document.getElementById('status-badge');
        badge.textContent = 'offline';
        badge.className = 'badge offline';
    }
}

function buildClientRow(c) {
    const tr = document.createElement('tr');
    tr.dataset.id = c.client_id;

    // Client ID
    const tdId = document.createElement('td');
    tdId.className = 'client-id';
    tdId.textContent = c.client_id;
    tr.appendChild(tdId);

    // Mode
    const tdMode = document.createElement('td');
    tdMode.className = 'client-mode';
    const modeSpan = document.createElement('span');
    tdMode.appendChild(modeSpan);
    tr.appendChild(tdMode);

    // Connected
    const tdConn = document.createElement('td');
    tdConn.className = 'client-connected';
    tr.appendChild(tdConn);

    // Requests
    const tdReq = document.createElement('td');
    tdReq.className = 'client-requests';
    tr.appendChild(tdReq);

    // Active
    const tdActive = document.createElement('td');
    tdActive.className = 'client-active';
    tr.appendChild(tdActive);

    // Data In
    const tdIn = document.createElement('td');
    tdIn.className = 'client-in';
    tr.appendChild(tdIn);

    // Data Out
    const tdOut = document.createElement('td');
    tdOut.className = 'client-out';
    tr.appendChild(tdOut);

    updateClientRow(tr, c);
    return tr;
}

function updateClientRow(tr, c) {
    // Mode
    const modeSpan = tr.querySelector('.client-mode span');
    if (modeSpan) {
        if (c.tcp_port) {
            modeSpan.style.color = 'var(--orange)';
            modeSpan.textContent = 'TCP:' + c.tcp_port;
        } else {
            modeSpan.style.color = 'var(--green)';
            modeSpan.textContent = 'HTTP';
        }
    }

    // Connected
    const tdConn = tr.querySelector('.client-connected');
    if (tdConn) {
        tdConn.textContent = formatDuration(c.connected_at_secs) + ' ago';
    }

    // Requests
    const tdReq = tr.querySelector('.client-requests');
    if (tdReq) {
        tdReq.textContent = formatNumber(c.total_requests);
    }

    // Active
    const tdActive = tr.querySelector('.client-active');
    if (tdActive) {
        tdActive.textContent = c.active_streams;
    }

    // Data In
    const tdIn = tr.querySelector('.client-in');
    if (tdIn) {
        tdIn.textContent = formatBytes(c.bytes_in);
    }

    // Data Out
    const tdOut = tr.querySelector('.client-out');
    if (tdOut) {
        tdOut.textContent = formatBytes(c.bytes_out);
    }
}

// ─── Init ───────────────────────────────────────────────────────

document.getElementById('login-form').addEventListener('submit', handleLogin);
document.getElementById('logout-btn').addEventListener('click', handleLogout);

// Check auth on page load
checkAuth();
