async function fetchJson(url, options = {}) {
    const response = await fetch(url, {
        headers: { 'Content-Type': 'application/json' },
        ...options,
    });
    if (!response.ok) {
        throw new Error(`Request failed: ${response.status}`);
    }
    return await response.json();
}

async function renderUpstreams() {
    try {
        const data = await fetchJson('/api/upstreams');
        const container = document.getElementById('upstreams');
        container.innerHTML = '';
        data.upstreams.forEach(([name, config]) => {
            const div = document.createElement('div');
            div.className = 'card';
            div.innerHTML = `<strong>${name}</strong><pre>${JSON.stringify(config, null, 2)}</pre>`;
            container.appendChild(div);
        });
    } catch (err) {
        console.error(err);
    }
}

async function renderProviders() {
    try {
        const data = await fetchJson('/api/providers');
        const container = document.getElementById('providers');
        container.innerHTML = '';
        data.providers.forEach(provider => {
            const div = document.createElement('div');
            div.className = 'card';
            div.textContent = `${provider.name} (${provider.kind})`;
            container.appendChild(div);
        });
    } catch (err) {
        console.error(err);
    }
}

async function renderUsers() {
    try {
        const data = await fetchJson('/api/users');
        const container = document.getElementById('users');
        container.innerHTML = '';
        data.users.forEach(user => {
            const div = document.createElement('div');
            div.className = 'card';
            div.textContent = `${user.email} - ${user.display_name ?? 'n/a'}`;
            container.appendChild(div);
        });
    } catch (err) {
        console.error(err);
    }
}

renderUpstreams();
renderProviders();
renderUsers();
