// Zo Tunnel Client Manager — Frontend Application
// Uses safe DOM methods only (no innerHTML, no document.write)

(function () {
    'use strict';

    var REFRESH_MS = 2000;
    var refreshInterval = null;
    var deleteTunnelId = null;

    // ─── Utilities ──────────────────────────────────────────────

    function formatDuration(secs) {
        if (secs == null) return '—';
        if (secs < 60) return secs + 's';
        if (secs < 3600) return Math.floor(secs / 60) + 'm ' + (secs % 60) + 's';
        var h = Math.floor(secs / 3600);
        var m = Math.floor((secs % 3600) / 60);
        return h + 'h ' + m + 'm';
    }

    function $(id) {
        return document.getElementById(id);
    }

    function createEl(tag, className, textContent) {
        var el = document.createElement(tag);
        if (className) el.className = className;
        if (textContent) el.textContent = textContent;
        return el;
    }

    async function api(method, url, body) {
        var opts = {
            method: method,
            headers: {}
        };
        if (body) {
            opts.headers['Content-Type'] = 'application/json';
            opts.body = JSON.stringify(body);
        }
        var resp = await fetch(url, opts);
        return resp.json();
    }

    // ─── View Switching ─────────────────────────────────────────

    function showSetup() {
        $('setup-screen').style.display = '';
        $('dashboard').style.display = 'none';
        stopRefresh();
    }

    function showDashboard() {
        $('setup-screen').style.display = 'none';
        $('dashboard').style.display = '';
        startRefresh();
    }

    function startRefresh() {
        if (refreshInterval) return;
        refreshDashboard();
        refreshInterval = setInterval(refreshDashboard, REFRESH_MS);
    }

    function stopRefresh() {
        if (refreshInterval) {
            clearInterval(refreshInterval);
            refreshInterval = null;
        }
    }

    // ─── Init: Check connection status ──────────────────────────

    async function init() {
        try {
            var resp = await api('GET', '/api/status');
            if (resp.success && resp.data.connected) {
                showDashboard();
                checkForUpgrades();
            } else {
                showSetup();
            }
        } catch (e) {
            showSetup();
        }
    }

    async function checkForUpgrades() {
        try {
            var resp = await api('GET', '/api/upgrade/check');
            if (resp.success && resp.data.upgrade_available) {
                $('upgrade-message').textContent = '🎉 A new version of Zo Tunnel Client is available: ' + resp.data.latest + ' (current: ' + resp.data.current + ')';
                $('btn-upgrade-client').textContent = 'Upgrade to ' + resp.data.latest;
                $('upgrade-banner').style.display = 'flex';
            } else {
                $('upgrade-banner').style.display = 'none';
            }
        } catch (e) {
            // Ignore upgrade check errors (e.g. rate limit or offline)
        }
    }

    // ─── Setup / Connect ────────────────────────────────────────

    $('setup-form').addEventListener('submit', async function (e) {
        e.preventDefault();

        var server = $('setup-server').value.trim();
        var token = $('setup-token').value.trim();

        if (!server) {
            $('setup-error').textContent = 'Server address is required';
            return;
        }
        if (!token) {
            $('setup-error').textContent = 'Token is required';
            return;
        }

        $('setup-error').textContent = '';
        $('btn-setup-connect').disabled = true;
        $('btn-setup-connect').textContent = 'Connecting...';

        try {
            var result = await api('POST', '/api/connect', { server: server, token: token });
            if (result.success) {
                showDashboard();
            } else {
                $('setup-error').textContent = result.error || 'Connection failed';
            }
        } catch (err) {
            $('setup-error').textContent = 'Connection error. Please check the server address.';
        } finally {
            $('btn-setup-connect').disabled = false;
            $('btn-setup-connect').textContent = 'Connect';
        }
    });



    // ─── Disconnect ─────────────────────────────────────────────

    $('btn-disconnect').addEventListener('click', function () {
        $('disconnect-overlay').classList.add('active');
    });

    $('btn-disconnect-cancel').addEventListener('click', function () {
        $('disconnect-overlay').classList.remove('active');
    });

    $('disconnect-close').addEventListener('click', function () {
        $('disconnect-overlay').classList.remove('active');
    });

    $('disconnect-overlay').addEventListener('click', function (e) {
        if (e.target === $('disconnect-overlay')) {
            $('disconnect-overlay').classList.remove('active');
        }
    });

    $('btn-disconnect-confirm').addEventListener('click', async function () {
        $('btn-disconnect-confirm').disabled = true;
        try {
            await api('POST', '/api/disconnect');
            $('disconnect-overlay').classList.remove('active');
            showSetup();
        } catch (e) {
            // ignore
        } finally {
            $('btn-disconnect-confirm').disabled = false;
        }
    });

    // ─── Dashboard Rendering ────────────────────────────────────

    function renderTunnels(data) {
        var container = $('tunnels-container');
        var emptyState = $('empty-state');

        if (data.length === 0) {
            // Remove existing tunnel cards
            var cards = container.querySelectorAll('.tunnel-card');
            for (var i = 0; i < cards.length; i++) {
                cards[i].remove();
            }

            if (!emptyState) {
                var es = createEl('div', 'empty-state');
                es.id = 'empty-state';
                var icon = createEl('div', 'empty-icon', '🚇');
                var title = createEl('h2', 'empty-title', 'No tunnels yet');
                var desc = createEl('p', 'empty-desc', 'Add your first tunnel to expose a local service to the internet');
                var btn = createEl('button', 'btn btn-primary');
                btn.id = 'btn-add-first';
                var plus = createEl('span', null, '+');
                btn.appendChild(plus);
                btn.appendChild(document.createTextNode(' Add Tunnel'));
                btn.addEventListener('click', openAddModal);
                es.appendChild(icon);
                es.appendChild(title);
                es.appendChild(desc);
                es.appendChild(btn);
                container.appendChild(es);
            } else {
                emptyState.style.display = '';
            }
            return;
        }

        if (emptyState) emptyState.style.display = 'none';

        // Reconcile tunnel cards to avoid visual flashing
        var existingCards = {};
        var cardElements = container.querySelectorAll('.tunnel-card');
        for (var i = 0; i < cardElements.length; i++) {
            var id = cardElements[i].dataset.id;
            if (id) {
                existingCards[id] = cardElements[i];
            }
        }

        var lastElement = null;
        data.forEach(function (t) {
            var existingCard = existingCards[t.id];
            var card;

            if (existingCard) {
                card = updateTunnelCard(existingCard, t);
                delete existingCards[t.id];
            } else {
                card = buildTunnelCard(t);
            }

            // Ensure cards preserve the order from API and avoid moving elements unnecessarily
            var currentSibling = lastElement ? lastElement.nextSibling : container.firstChild;
            while (currentSibling && !currentSibling.classList.contains('tunnel-card')) {
                currentSibling = currentSibling.nextSibling;
            }

            if (currentSibling !== card) {
                container.insertBefore(card, currentSibling);
            }
            lastElement = card;
        });

        // Remove cards that are no longer present
        for (var id in existingCards) {
            existingCards[id].remove();
        }
    }

    function buildTunnelCard(t) {
        var state = t.status.state;
        var localOk = t.status.local_reachable;
        var card = createEl('div', 'tunnel-card ' + state);
        card.dataset.id = t.id;

        var top = createEl('div', 'tunnel-card-top');
        var info = createEl('div', 'tunnel-info');

        // Name row
        var nameRow = createEl('div', 'tunnel-name');
        nameRow.appendChild(createEl('span', 'status-dot ' + state));
        nameRow.appendChild(document.createTextNode(t.client_id));
        info.appendChild(nameRow);

        // Route
        if (t.status.route) {
            info.appendChild(createEl('div', 'tunnel-route', t.status.route));
        }

        // Local address
        var local = createEl('div', 'tunnel-local');
        local.appendChild(createEl('span', 'tunnel-local-arrow', '→'));
        local.appendChild(document.createTextNode(' ' + t.local_addr));
        info.appendChild(local);

        // Uptime
        if (state === 'connected' && t.status.connected_since_secs != null) {
            var meta = createEl('div', 'tunnel-meta');
            meta.appendChild(createEl('span', 'tunnel-meta-item', '⏱ ' + formatDuration(t.status.connected_since_secs)));
            info.appendChild(meta);
        }

        // Error
        if (state === 'error' && t.status.error) {
            info.appendChild(createEl('div', 'tunnel-error', t.status.error));
        }

        top.appendChild(info);

        // ── Dual Status Tags ──
        var statusTags = createEl('div', 'status-tags');

        // Server status
        var serverTag = createEl('span', 'tunnel-status-tag ' + state);
        serverTag.appendChild(createEl('span', 'status-dot ' + state));
        serverTag.appendChild(document.createTextNode('Server: ' + stateLabel(state)));
        statusTags.appendChild(serverTag);

        // Local service status
        var localClass = localOk ? 'local-up' : 'local-down';
        var localTag = createEl('span', 'tunnel-status-tag ' + localClass);
        localTag.appendChild(createEl('span', 'status-dot ' + localClass));
        localTag.appendChild(document.createTextNode('Local: ' + (localOk ? 'Up' : 'Down')));
        statusTags.appendChild(localTag);

        top.appendChild(statusTags);

        card.appendChild(top);

        // Actions
        var bottom = createEl('div', 'tunnel-card-bottom');

        var btnEdit = createEl('button', 'btn btn-secondary btn-sm', '✏️ Edit');
        btnEdit.addEventListener('click', function () { openEditModal(t); });
        bottom.appendChild(btnEdit);

        if (state === 'connected' || state === 'connecting') {
            var btnStop = createEl('button', 'btn btn-warning btn-sm', '⏹ Stop');
            btnStop.addEventListener('click', function () { actionTunnel(t.id, 'stop'); });
            bottom.appendChild(btnStop);

            var btnRestart = createEl('button', 'btn btn-secondary btn-sm', '🔄 Restart');
            btnRestart.addEventListener('click', function () { actionTunnel(t.id, 'restart'); });
            bottom.appendChild(btnRestart);
        } else {
            var btnStart = createEl('button', 'btn btn-success btn-sm', '▶ Start');
            btnStart.addEventListener('click', function () { actionTunnel(t.id, 'start'); });
            bottom.appendChild(btnStart);
        }

        var btnDel = createEl('button', 'btn btn-danger btn-sm', '🗑 Delete');
        btnDel.addEventListener('click', function () { openDeleteModal(t); });
        bottom.appendChild(btnDel);

        card.appendChild(bottom);
        return card;
    }

    function updateTunnelCard(card, t) {
        var state = t.status.state;
        var localOk = t.status.local_reachable;

        card.className = 'tunnel-card ' + state;

        var nameDot = card.querySelector('.tunnel-name .status-dot');
        if (nameDot) nameDot.className = 'status-dot ' + state;

        var routeEl = card.querySelector('.tunnel-route');
        if (t.status.route) {
            if (routeEl) {
                routeEl.textContent = t.status.route;
            } else {
                var newRoute = createEl('div', 'tunnel-route', t.status.route);
                var localEl = card.querySelector('.tunnel-local');
                localEl.parentNode.insertBefore(newRoute, localEl);
            }
        } else {
            if (routeEl) routeEl.remove();
        }

        var localEl = card.querySelector('.tunnel-local');
        if (localEl) {
            localEl.replaceChildren();
            localEl.appendChild(createEl('span', 'tunnel-local-arrow', '→'));
            localEl.appendChild(document.createTextNode(' ' + t.local_addr));
        }

        var metaEl = card.querySelector('.tunnel-meta');
        if (state === 'connected' && t.status.connected_since_secs != null) {
            var text = '⏱ ' + formatDuration(t.status.connected_since_secs);
            if (metaEl) {
                var itemEl = metaEl.querySelector('.tunnel-meta-item');
                if (itemEl) itemEl.textContent = text;
            } else {
                var newMeta = createEl('div', 'tunnel-meta');
                newMeta.appendChild(createEl('span', 'tunnel-meta-item', text));
                var localEl2 = card.querySelector('.tunnel-local');
                localEl2.parentNode.insertBefore(newMeta, localEl2.nextSibling);
            }
        } else {
            if (metaEl) metaEl.remove();
        }

        var errorEl = card.querySelector('.tunnel-error');
        if (state === 'error' && t.status.error) {
            if (errorEl) {
                errorEl.textContent = t.status.error;
            } else {
                var newError = createEl('div', 'tunnel-error', t.status.error);
                var insertAfter = card.querySelector('.tunnel-meta') || card.querySelector('.tunnel-local');
                insertAfter.parentNode.insertBefore(newError, insertAfter.nextSibling);
            }
        } else {
            if (errorEl) errorEl.remove();
        }

        var statusTags = card.querySelector('.status-tags');
        if (statusTags) {
            statusTags.replaceChildren();

            var serverTag = createEl('span', 'tunnel-status-tag ' + state);
            serverTag.appendChild(createEl('span', 'status-dot ' + state));
            serverTag.appendChild(document.createTextNode('Server: ' + stateLabel(state)));
            statusTags.appendChild(serverTag);

            var localClass = localOk ? 'local-up' : 'local-down';
            var localTag = createEl('span', 'tunnel-status-tag ' + localClass);
            localTag.appendChild(createEl('span', 'status-dot ' + localClass));
            localTag.appendChild(document.createTextNode('Local: ' + (localOk ? 'Up' : 'Down')));
            statusTags.appendChild(localTag);
        }

        var bottom = card.querySelector('.tunnel-card-bottom');
        if (bottom) {
            bottom.replaceChildren();

            var btnEdit = createEl('button', 'btn btn-secondary btn-sm', '✏️ Edit');
            btnEdit.addEventListener('click', function () { openEditModal(t); });
            bottom.appendChild(btnEdit);

            if (state === 'connected' || state === 'connecting') {
                var btnStop = createEl('button', 'btn btn-warning btn-sm', '⏹ Stop');
                btnStop.addEventListener('click', function () { actionTunnel(t.id, 'stop'); });
                bottom.appendChild(btnStop);

                var btnRestart = createEl('button', 'btn btn-secondary btn-sm', '🔄 Restart');
                btnRestart.addEventListener('click', function () { actionTunnel(t.id, 'restart'); });
                bottom.appendChild(btnRestart);
            } else {
                var btnStart = createEl('button', 'btn btn-success btn-sm', '▶ Start');
                btnStart.addEventListener('click', function () { actionTunnel(t.id, 'start'); });
                bottom.appendChild(btnStart);
            }

            var btnDel = createEl('button', 'btn btn-danger btn-sm', '🗑 Delete');
            btnDel.addEventListener('click', function () { openDeleteModal(t); });
            bottom.appendChild(btnDel);
        }

        return card;
    }

    function stateLabel(state) {
        switch (state) {
            case 'connected': return 'Online';
            case 'connecting': return 'Connecting';
            case 'error': return 'Error';
            case 'stopped': return 'Stopped';
            default: return state;
        }
    }

    // ─── Data Fetching ──────────────────────────────────────────

    async function refreshDashboard() {
        try {
            var results = await Promise.all([
                api('GET', '/api/tunnels'),
                api('GET', '/api/status')
            ]);

            var tunnelsResp = results[0];
            var statusResp = results[1];

            // If no longer connected, switch to setup
            if (statusResp.success && !statusResp.data.connected) {
                showSetup();
                return;
            }

            if (tunnelsResp.success) {
                renderTunnels(tunnelsResp.data);
            }

            if (statusResp.success) {
                updateHeader(statusResp.data);
            }
        } catch (e) {
            // Connection to local server lost
        }
    }

    function updateHeader(status) {
        $('server-info').textContent = status.server || '—';
        $('version-info').textContent = 'v' + (status.version || '');
        $('stat-total').textContent = status.total_tunnels;
        $('stat-running').textContent = status.running_tunnels;
    }

    // ─── Tunnel Actions ─────────────────────────────────────────

    async function actionTunnel(id, action) {
        try {
            await api('POST', '/api/tunnels/' + encodeURIComponent(id) + '/' + action);
            setTimeout(refreshDashboard, 300);
        } catch (e) { /* shown in next refresh */ }
    }

    // ─── Add/Edit Modal ─────────────────────────────────────────

    function openAddModal() {
        $('modal-title').textContent = 'Add Tunnel';
        $('btn-submit').textContent = 'Add Tunnel';
        $('form-id').value = '';
        $('form-client-id').value = '';
        $('form-local-addr').value = '';
        $('form-enabled').checked = true;
        $('form-error').textContent = '';
        $('modal-overlay').classList.add('active');
        $('form-client-id').focus();
    }

    function openEditModal(tunnel) {
        $('modal-title').textContent = 'Edit Tunnel';
        $('btn-submit').textContent = 'Save Changes';
        $('form-id').value = tunnel.id;
        $('form-client-id').value = tunnel.client_id;
        $('form-local-addr').value = tunnel.local_addr;
        $('form-enabled').checked = tunnel.enabled;
        $('form-error').textContent = '';
        $('modal-overlay').classList.add('active');
        $('form-local-addr').focus();
    }

    function closeModal() {
        $('modal-overlay').classList.remove('active');
    }

    $('tunnel-form').addEventListener('submit', async function (e) {
        e.preventDefault();

        var id = $('form-id').value;
        var clientId = $('form-client-id').value.trim();
        var localAddr = $('form-local-addr').value.trim();
        var enabled = $('form-enabled').checked;

        if (!clientId) { $('form-error').textContent = 'Tunnel name is required'; return; }
        if (!/^[a-zA-Z0-9_-]+$/.test(clientId)) { $('form-error').textContent = 'Name can only contain letters, numbers, hyphens, and underscores'; return; }
        if (!localAddr) { $('form-error').textContent = 'Local address is required'; return; }

        $('form-error').textContent = '';
        $('btn-submit').disabled = true;

        try {
            var result;
            if (id) {
                result = await api('PUT', '/api/tunnels/' + encodeURIComponent(id), {
                    client_id: clientId, local_addr: localAddr, enabled: enabled
                });
            } else {
                result = await api('POST', '/api/tunnels', {
                    client_id: clientId, local_addr: localAddr, enabled: enabled
                });
            }

            if (result.success) {
                closeModal();
                refreshDashboard();
            } else {
                $('form-error').textContent = result.error || 'Operation failed';
            }
        } catch (err) {
            $('form-error').textContent = 'Connection error. Please try again.';
        } finally {
            $('btn-submit').disabled = false;
        }
    });

    $('btn-cancel').addEventListener('click', closeModal);
    $('modal-close').addEventListener('click', closeModal);
    $('modal-overlay').addEventListener('click', function (e) {
        if (e.target === $('modal-overlay')) closeModal();
    });

    // ─── Delete Modal ───────────────────────────────────────────

    function openDeleteModal(tunnel) {
        deleteTunnelId = tunnel.id;
        $('delete-name').textContent = tunnel.client_id;
        var deleteError = $('delete-error');
        if (deleteError) deleteError.textContent = '';
        $('delete-overlay').classList.add('active');
    }

    function closeDeleteModal() {
        $('delete-overlay').classList.remove('active');
        deleteTunnelId = null;
    }

    $('btn-delete-cancel').addEventListener('click', closeDeleteModal);
    $('delete-close').addEventListener('click', closeDeleteModal);
    $('delete-overlay').addEventListener('click', function (e) {
        if (e.target === $('delete-overlay')) closeDeleteModal();
    });

    $('btn-delete-confirm').addEventListener('click', async function () {
        if (!deleteTunnelId) return;
        $('btn-delete-confirm').disabled = true;
        var deleteError = $('delete-error');
        if (deleteError) deleteError.textContent = '';
        try {
            var result = await api('DELETE', '/api/tunnels/' + encodeURIComponent(deleteTunnelId));
            if (result.success) { 
                closeDeleteModal(); 
                refreshDashboard(); 
            } else {
                if (deleteError) deleteError.textContent = result.error || 'Failed to delete tunnel';
            }
        } catch (e) { 
            if (deleteError) deleteError.textContent = 'Connection error. Please try again.';
        } finally { 
            $('btn-delete-confirm').disabled = false; 
        }
    });

    // ─── Misc ───────────────────────────────────────────────────

    $('form-client-id').addEventListener('input', function () {
        $('subdomain-preview').textContent = (this.value.trim() || 'name') + '.your-domain';
    });

    $('btn-upgrade-client').addEventListener('click', async function () {
        if (!confirm('Are you sure you want to upgrade the client to the latest version? This will temporarily disconnect your tunnels.')) {
            return;
        }

        $('upgrade-overlay').classList.add('active');
        $('upgrade-progress-text').textContent = 'Downloading and installing the latest version...';

        try {
            var resp = await api('POST', '/api/upgrade');
            if (resp.success) {
                $('upgrade-progress-text').textContent = 'Upgrade successful! Restarting client...';
                
                // Wait for the client to go offline, then come back online
                setTimeout(pollRestartAndReload, 1500);
            } else {
                $('upgrade-overlay').classList.remove('active');
                alert('Upgrade failed: ' + (resp.error || 'Unknown error'));
            }
        } catch (err) {
            $('upgrade-overlay').classList.remove('active');
            alert('Upgrade error: ' + err.message);
        }
    });

    async function pollRestartAndReload() {
        var attempts = 0;
        var interval = setInterval(async function () {
            attempts++;
            if (attempts > 30) {
                clearInterval(interval);
                $('upgrade-progress-text').textContent = 'Client restart is taking longer than expected. Please reload the page manually.';
                return;
            }

            try {
                var resp = await api('GET', '/api/status');
                if (resp.success) {
                    clearInterval(interval);
                    $('upgrade-progress-text').textContent = 'Reconnected! Reloading page...';
                    setTimeout(function () {
                        window.location.reload();
                    }, 500);
                }
            } catch (e) {
                // Ignore connection errors while server is restarting
            }
        }, 1000);
    }

    $('btn-add-tunnel').addEventListener('click', openAddModal);
    var addFirstBtn = $('btn-add-first');
    if (addFirstBtn) addFirstBtn.addEventListener('click', openAddModal);

    document.addEventListener('keydown', function (e) {
        if (e.key === 'Escape') {
            closeModal();
            closeDeleteModal();
            $('disconnect-overlay').classList.remove('active');
        }
    });

    // ─── Start ──────────────────────────────────────────────────
    init();

})();
