const navButtons = document.querySelectorAll('nav button');
const views = document.querySelectorAll('.view');

navButtons.forEach((button) => {
    button.addEventListener('click', () => {
        const target = button.dataset.view;
        views.forEach((view) => {
            view.classList.toggle('active', view.id === `view-${target}`);
        });
        if (target === 'dashboard') {
            loadHealth();
            loadMetrics();
        }
    });
});

document.getElementById('refresh-upstreams').addEventListener('click', loadUpstreams);
document.getElementById('load-tools').addEventListener('click', loadTools);
document.getElementById('load-resources').addEventListener('click', loadResources);
document.getElementById('load-prompts').addEventListener('click', loadPrompts);

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
    list.innerHTML = '';
    const upstreams = ['stub'];
    upstreams.forEach((item) => {
        const li = document.createElement('li');
        li.textContent = item;
        list.appendChild(li);
    });
}

async function loadTools() {
    const list = document.getElementById('tool-list');
    list.innerHTML = '';
    const response = await fetch('/mcp', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/list' })
    });
    const data = await response.json();
    const tools = data.result?.tools ?? [];
    tools.forEach((tool) => {
        const li = document.createElement('li');
        li.textContent = `${tool.name}: ${tool.description}`;
        list.appendChild(li);
    });
}

async function loadResources() {
    const list = document.getElementById('resource-list');
    list.innerHTML = '';
    const response = await fetch('/mcp', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'resources/list' })
    });
    const data = await response.json();
    const resources = data.result?.resources ?? [];
    resources.forEach((resource) => {
        const li = document.createElement('li');
        li.textContent = `${resource.name} (${resource.uri})`;
        list.appendChild(li);
    });
}

async function loadPrompts() {
    const list = document.getElementById('prompt-list');
    list.innerHTML = '';
    const response = await fetch('/mcp', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'prompts/list' })
    });
    const data = await response.json();
    const prompts = data.result?.prompts ?? [];
    prompts.forEach((prompt) => {
        const li = document.createElement('li');
        li.textContent = `${prompt.name}: ${prompt.description}`;
        list.appendChild(li);
    });
}

loadHealth();
loadMetrics();
