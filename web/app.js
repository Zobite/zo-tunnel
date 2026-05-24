// Zobite Tunnel Dashboard — auto-refreshing client

const REFRESH_MS = 2000;

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

async function fetchJSON(url) {
    const resp = await fetch(url);
    if (!resp.ok) throw new Error(resp.statusText);
    return resp.json();
}

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

        // Clients table
        const tbody = document.getElementById('clients-body');
        if (clients.length === 0) {
            tbody.innerHTML = '<tr><td colspan="6" class="empty">No clients connected</td></tr>';
        } else {
            tbody.innerHTML = clients.map(c => `
                <tr>
                    <td class="client-id">${escapeHtml(c.client_id)}</td>
                    <td>${formatDuration(c.connected_at_secs)} ago</td>
                    <td>${formatNumber(c.total_requests)}</td>
                    <td>${c.active_streams}</td>
                    <td>${formatBytes(c.bytes_in)}</td>
                    <td>${formatBytes(c.bytes_out)}</td>
                </tr>
            `).join('');
        }

    } catch (e) {
        const badge = document.getElementById('status-badge');
        badge.textContent = 'offline';
        badge.className = 'badge offline';
    }
}

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

// Initial load + interval
refresh();
setInterval(refresh, REFRESH_MS);
