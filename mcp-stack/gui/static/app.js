const navButtons = document.querySelectorAll('nav button');
const views = document.querySelectorAll('.view');
const authInput = document.getElementById('auth-token');
const authStatus = document.getElementById('auth-status');
let authToken = localStorage.getItem('mcpAuthToken') || '';

if (authInput) {
    authInput.value = authToken;
}

function setStatus(id, message, state = 'info') {
    const el = document.getElementById(id);
    if (!el) {
        return;
    }
    el.textContent = message;
    if (message) {
        el.dataset.state = state;
    } else {
        delete el.dataset.state;
    }
}

function updateAuthToken(value) {
    authToken = value.trim();
    if (authToken) {
        localStorage.setItem('mcpAuthToken', authToken);
        setStatus('auth-status', 'Token saved locally.', 'success');
    } else {
        localStorage.removeItem('mcpAuthToken');
        setStatus('auth-status', 'Token cleared.', 'info');
    }
    if (authToken) {
        refreshAdminData();
    }
}

if (authInput) {
    authInput.addEventListener('change', (event) => updateAuthToken(event.target.value));
}

const clearToken = document.getElementById('clear-token');
if (clearToken) {
    clearToken.addEventListener('click', () => {
        if (authInput) {
            authInput.value = '';
        }
        updateAuthToken('');
    });
}

navButtons.forEach((button) => {
    button.addEventListener('click', () => {
        const target = button.dataset.view;
        navButtons.forEach((btn) => btn.classList.toggle('active', btn === button));
        views.forEach((view) => {
            view.classList.toggle('active', view.id === `view-${target}`);
        });
        switch (target) {
            case 'dashboard':
                loadHealth();
                loadMetrics();
                break;
            case 'upstreams':
                loadUpstreams();
                break;
            case 'providers':
                loadProviders();
                break;
            case 'subscriptions':
                loadSubscriptions();
                loadUsers();
                break;
            case 'users':
                loadUsers();
                loadTokens();
                break;
            case 'tools':
                loadTools();
                break;
            case 'resources':
                loadResources();
                break;
            case 'prompts':
                loadPrompts();
                break;
        }
    });
});

if (navButtons.length > 0) {
    navButtons[0].classList.add('active');
}

document.getElementById('refresh-upstreams')?.addEventListener('click', loadUpstreams);
document.getElementById('refresh-providers')?.addEventListener('click', loadProviders);
document.getElementById('refresh-subscriptions')?.addEventListener('click', loadSubscriptions);
document.getElementById('refresh-users')?.addEventListener('click', () => {
    loadUsers();
    loadTokens();
});

document.getElementById('load-tools')?.addEventListener('click', loadTools);
document.getElementById('load-resources')?.addEventListener('click', loadResources);
document.getElementById('load-prompts')?.addEventListener('click', loadPrompts);

document.getElementById('upstream-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.target);
    const argsValue = (formData.get('args') || '').toString().trim();
    const payload = {
        name: formData.get('name'),
        kind: formData.get('kind'),
        command: formData.get('command')?.toString().trim() || null,
        args: argsValue ? argsValue.split(/\s+/) : [],
        url: formData.get('url')?.toString().trim() || null,
        bearer: formData.get('bearer')?.toString().trim() || null,
        provider_slug: formData.get('provider_slug')?.toString().trim() || null,
    };
    try {
        const response = await fetchJson('/api/upstreams', {
            method: 'POST',
            body: JSON.stringify(payload),
        });
        setStatus('upstream-status', `Registered upstream ${response?.name ?? payload.name}.`, 'success');
        event.target.reset();
        loadUpstreams();
    } catch (err) {
        setStatus('upstream-status', err.message, 'error');
    }
});

document.getElementById('provider-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.target);
    const payload = {
        slug: formData.get('slug'),
        display_name: formData.get('display_name'),
        description: formData.get('description')?.toString().trim() || null,
    };
    try {
        const provider = await fetchJson('/api/providers', {
            method: 'POST',
            body: JSON.stringify(payload),
        });
        setStatus('provider-status', `Saved provider ${provider.slug}.`, 'success');
        event.target.reset();
        loadProviders();
    } catch (err) {
        setStatus('provider-status', err.message, 'error');
    }
});

document.getElementById('provider-key-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.target);
    const payload = {
        provider_slug: formData.get('provider_slug'),
        name: formData.get('name'),
        value: formData.get('value'),
    };
    try {
        await fetchJson('/api/providers/keys', {
            method: 'POST',
            body: JSON.stringify(payload),
        });
        setStatus('provider-key-status', 'Stored provider key.', 'success');
        event.target.reset();
    } catch (err) {
        setStatus('provider-key-status', err.message, 'error');
    }
});

document.getElementById('subscription-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.target);
    const payload = {
        user_id: formData.get('user_id'),
        tier: formData.get('tier'),
    };
    const maxTokens = formData.get('max_tokens');
    const maxRequests = formData.get('max_requests');
    const maxConcurrent = formData.get('max_concurrent');
    if (maxTokens) payload.max_tokens = Number(maxTokens);
    if (maxRequests) payload.max_requests = Number(maxRequests);
    if (maxConcurrent) payload.max_concurrent = Number(maxConcurrent);
    try {
        await fetchJson('/api/subscriptions', {
            method: 'POST',
            body: JSON.stringify(payload),
        });
        setStatus('subscription-status', 'Subscription updated.', 'success');
        event.target.reset();
        loadSubscriptions();
    } catch (err) {
        setStatus('subscription-status', err.message, 'error');
    }
});

document.getElementById('user-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.target);
    const payload = {
        email: formData.get('email'),
        name: formData.get('name')?.toString().trim() || null,
    };
    try {
        await fetchJson('/api/users', {
            method: 'POST',
            body: JSON.stringify(payload),
        });
        setStatus('user-status', 'User ensured.', 'success');
        event.target.reset();
        loadUsers();
    } catch (err) {
        setStatus('user-status', err.message, 'error');
    }
});

document.getElementById('token-form')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const formData = new FormData(event.target);
    const payload = {
        user_id: formData.get('user_id'),
        scope: formData.get('scope')?.toString().trim() || undefined,
    };
    try {
        await fetchJson('/api/tokens', {
            method: 'POST',
            body: JSON.stringify(payload),
        });
        setStatus('token-status', 'Issued new token.', 'success');
        event.target.reset();
        loadTokens();
    } catch (err) {
        setStatus('token-status', err.message, 'error');
    }
});

async function authorizedFetch(url, options = {}) {
    const opts = { ...options };
    const headers = new Headers(options.headers || {});
    if (!headers.has('Accept')) {
        headers.set('Accept', 'application/json');
    }
    if (opts.body && !headers.has('Content-Type')) {
        headers.set('Content-Type', 'application/json');
    }
    if (authToken) {
        headers.set('Authorization', `Bearer ${authToken}`);
    }
    opts.headers = headers;
    return fetch(url, opts);
}

async function fetchJson(url, options = {}) {
    const response = await authorizedFetch(url, options);
    if (response.status === 204) {
        return null;
    }
    if (!response.ok) {
        if (response.status === 401) {
            throw new Error('Unauthorized - set or update the admin token.');
        }
        const text = await response.text();
        throw new Error(text || `HTTP ${response.status}`);
    }
    return response.json();
}

async function loadHealth() {
    const status = document.getElementById('health-status');
    try {
        const res = await fetch('/healthz');
        status.textContent = res.ok ? 'Router healthy' : `Error: ${res.status}`;
    } catch (err) {
        status.textContent = `Error: ${err}`;
    }
}

async function loadMetrics() {
    const output = document.getElementById('metrics-output');
    try {
        const res = await fetch('/metrics');
        output.textContent = await res.text();
    } catch (err) {
        output.textContent = `Error: ${err}`;
    }
}

async function loadUpstreams() {
    const list = document.getElementById('upstream-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const upstreams = (await fetchJson('/api/upstreams')) || [];
        if (!Array.isArray(upstreams) || upstreams.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No upstreams registered yet.';
            list.appendChild(li);
        } else {
            upstreams.forEach((upstream) => {
                const li = document.createElement('li');
                const args = upstream.args?.length ? ` args=[${upstream.args.join(' ')}]` : '';
                li.textContent = `${upstream.name} (${upstream.kind})${args}`;
                list.appendChild(li);
            });
        }
        setStatus('upstream-status', `Loaded ${upstreams.length} upstream(s).`, 'success');
    } catch (err) {
        setStatus('upstream-status', err.message, 'error');
        const li = document.createElement('li');
        li.textContent = `Error loading upstreams: ${err.message}`;
        list.appendChild(li);
    }
}

async function loadTools() {
    const list = document.getElementById('tool-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const data = await fetchJson('/mcp', {
            method: 'POST',
            body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list' }),
        });
        const tools = data?.result?.tools ?? [];
        if (tools.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No tools available.';
            list.appendChild(li);
        } else {
            tools.forEach((tool) => {
                const li = document.createElement('li');
                li.textContent = `${tool.name}: ${tool.description ?? ''}`;
                list.appendChild(li);
            });
        }
    } catch (err) {
        const li = document.createElement('li');
        li.textContent = `Error loading tools: ${err.message}`;
        list.appendChild(li);
    }
}

async function loadResources() {
    const list = document.getElementById('resource-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const data = await fetchJson('/mcp', {
            method: 'POST',
            body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'resources/list' }),
        });
        const resources = data?.result?.resources ?? [];
        if (resources.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No resources available.';
            list.appendChild(li);
        } else {
            resources.forEach((resource) => {
                const li = document.createElement('li');
                li.textContent = `${resource.name} (${resource.uri})`;
                list.appendChild(li);
            });
        }
    } catch (err) {
        const li = document.createElement('li');
        li.textContent = `Error loading resources: ${err.message}`;
        list.appendChild(li);
    }
}

async function loadPrompts() {
    const list = document.getElementById('prompt-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const data = await fetchJson('/mcp', {
            method: 'POST',
            body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'prompts/list' }),
        });
        const prompts = data?.result?.prompts ?? [];
        if (prompts.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No prompts available.';
            list.appendChild(li);
        } else {
            prompts.forEach((prompt) => {
                const li = document.createElement('li');
                li.textContent = `${prompt.name}: ${prompt.description ?? ''}`;
                list.appendChild(li);
            });
        }
    } catch (err) {
        const li = document.createElement('li');
        li.textContent = `Error loading prompts: ${err.message}`;
        list.appendChild(li);
    }
}

async function loadProviders() {
    const list = document.getElementById('provider-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const providers = (await fetchJson('/api/providers')) || [];
        if (!Array.isArray(providers) || providers.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No providers registered.';
            list.appendChild(li);
        } else {
            providers.forEach((provider) => {
                const li = document.createElement('li');
                li.textContent = `${provider.display_name} (${provider.slug})`;
                if (provider.description) {
                    const small = document.createElement('div');
                    small.textContent = provider.description;
                    small.style.fontSize = '0.85rem';
                    small.style.opacity = '0.8';
                    li.appendChild(small);
                }
                list.appendChild(li);
            });
        }
        setStatus('provider-status', '', 'info');
    } catch (err) {
        setStatus('provider-status', err.message, 'error');
        const li = document.createElement('li');
        li.textContent = `Error loading providers: ${err.message}`;
        list.appendChild(li);
    }
}

async function loadSubscriptions() {
    const list = document.getElementById('subscription-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const subscriptions = (await fetchJson('/api/subscriptions')) || [];
        if (!Array.isArray(subscriptions) || subscriptions.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No subscriptions assigned.';
            list.appendChild(li);
        } else {
            subscriptions.forEach((record) => {
                const li = document.createElement('li');
                const expiry = record.expires_at ? `, expires ${record.expires_at}` : '';
                li.textContent = `${record.user_id}: ${record.tier} (${record.tokens_used}/${record.max_tokens} tokens, ${record.requests_used}/${record.max_requests} requests${expiry})`;
                list.appendChild(li);
            });
        }
        setStatus('subscription-status', '', 'info');
    } catch (err) {
        setStatus('subscription-status', err.message, 'error');
        const li = document.createElement('li');
        li.textContent = `Error loading subscriptions: ${err.message}`;
        list.appendChild(li);
    }
}

async function loadUsers() {
    const list = document.getElementById('user-list');
    const subscriptionSelect = document.getElementById('subscription-user-id');
    const tokenSelect = document.getElementById('token-user-id');
    if (!list || !subscriptionSelect || !tokenSelect) return;
    list.innerHTML = '';
    subscriptionSelect.innerHTML = '';
    tokenSelect.innerHTML = '';
    try {
        const users = (await fetchJson('/api/users')) || [];
        if (!Array.isArray(users) || users.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No users found.';
            list.appendChild(li);
            addEmptyOption(subscriptionSelect, 'No users available');
            addEmptyOption(tokenSelect, 'No users available');
        } else {
            users.forEach((user) => {
                const li = document.createElement('li');
                li.textContent = `${user.email}${user.name ? ` (${user.name})` : ''}`;
                list.appendChild(li);
            });
            populateUserSelect(subscriptionSelect, users);
            populateUserSelect(tokenSelect, users);
        }
        setStatus('user-status', '', 'info');
    } catch (err) {
        setStatus('user-status', err.message, 'error');
        const li = document.createElement('li');
        li.textContent = `Error loading users: ${err.message}`;
        list.appendChild(li);
        addEmptyOption(subscriptionSelect, 'Unable to load users');
        addEmptyOption(tokenSelect, 'Unable to load users');
    }
}

async function loadTokens() {
    const list = document.getElementById('token-list');
    if (!list) return;
    list.innerHTML = '';
    try {
        const tokens = (await fetchJson('/api/tokens')) || [];
        if (!Array.isArray(tokens) || tokens.length === 0) {
            const li = document.createElement('li');
            li.textContent = 'No tokens issued yet.';
            list.appendChild(li);
        } else {
            tokens.forEach((token) => {
                const li = document.createElement('li');
                li.textContent = `${token.user_id}: ${token.token} [${token.scope}]`;
                list.appendChild(li);
            });
        }
        setStatus('token-status', '', 'info');
    } catch (err) {
        setStatus('token-status', err.message, 'error');
        const li = document.createElement('li');
        li.textContent = `Error loading tokens: ${err.message}`;
        list.appendChild(li);
    }
}

function populateUserSelect(select, users) {
    addEmptyOption(select, 'Select a user', false);
    users.forEach((user, index) => {
        const option = document.createElement('option');
        option.value = user.id;
        option.textContent = user.email;
        if (index === 0) {
            option.selected = true;
        }
        select.appendChild(option);
    });
}

function addEmptyOption(select, label, disabled = true) {
    const option = document.createElement('option');
    option.value = '';
    option.textContent = label;
    option.disabled = disabled;
    option.selected = true;
    select.appendChild(option);
}

function refreshAdminData() {
    loadUpstreams();
    loadProviders();
    loadSubscriptions();
    loadUsers();
    loadTokens();
}

loadHealth();
loadMetrics();
if (authToken) {
    setStatus('auth-status', 'Token loaded from browser storage.', 'info');
    refreshAdminData();
}
