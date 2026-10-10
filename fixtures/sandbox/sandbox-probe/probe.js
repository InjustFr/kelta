// Gate S1 probes (ARCHITECTURE §11.3). Runs inside a plugin screen iframe and a localhost web tool
// iframe; every escape attempt must fail. Results go to <body data-s1="{json}"> for run.mjs.
const out = {};
const sync = (name, fn) => {
  try {
    out[name] = String(fn());
  } catch (e) {
    out[name] = `threw ${e.name}`;
  }
};

sync('internals', () => typeof window.__TAURI_INTERNALS__);
sync('tauri', () => typeof window.__TAURI__);
sync('parentInternals', () => typeof window.parent.__TAURI_INTERNALS__);
sync('topInternals', () => typeof window.top.__TAURI_INTERNALS__);
sync('parentRead', () => window.parent.document.title);
sync('parentWrite', () => {
  window.parent.document.body.dataset.pwned = 'parent';
  return 'wrote';
});
sync('topWrite', () => {
  window.top.document.body.dataset.pwned = 'top';
  return 'wrote';
});

// Self-escalation through a forged IPC message: run.mjs checks the grants did not change.
const grant = { id: 'sandbox-probe', permissions: ['projects.read', 'tickets.read', 'ui.open'] };
sync('webkitIpc', () => {
  window.webkit.messageHandlers.ipc.postMessage(
    JSON.stringify({ cmd: 'plugin_grant', callback: 1, error: 2, payload: grant, __TAURI_INVOKE_KEY__: '0' }),
  );
  return 'posted';
});

async function fetchIpc(url) {
  try {
    const r = await fetch(url, {
      method: 'POST',
      signal: AbortSignal.timeout(3000), // an unanswered request counts as rejected
      body: JSON.stringify(grant),
      headers: {
        'Content-Type': 'application/json',
        'Tauri-Callback': '1',
        'Tauri-Error': '2',
        'Tauri-Invoke-Key': '0',
      },
    });
    return `status ${r.status}`;
  } catch (e) {
    return `threw ${e.name}`;
  }
}

// The host bridge (PLUGINS §7): one MessageChannel, answered only for plugin screens.
function connect() {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), 3000);
    addEventListener('message', (ev) => {
      if (ev.data?.type !== 'kelta:init' || !ev.ports[0]) return;
      clearTimeout(timer);
      resolve(ev.ports[0]);
    });
    window.parent.postMessage({ type: 'kelta:ready' }, '*');
  });
}

let nextId = 1;
function call(port, method) {
  if (!port) return 'no bridge';
  const id = nextId++;
  return new Promise((resolve) => {
    port.addEventListener('message', (m) => {
      if (m.data?.id === id) resolve(m.data.error ? m.data.error.code : 'ok');
    });
    port.start();
    port.postMessage({ id, method, params: {} });
  });
}

async function main() {
  out.fetchIpcScheme = await fetchIpc('ipc://localhost/plugin_grant');
  out.fetchIpcHttp = await fetchIpc('http://ipc.localhost/plugin_grant');
  const port = await connect();
  out.granted = await call(port, 'projects.list');
  out.notGranted = await call(port, 'tickets.list');
  document.body.dataset.s1 = JSON.stringify(out);
  document.getElementById('out').textContent = JSON.stringify(out, null, 2);
  // Last: if this worked the whole app would be gone; run.mjs checks the top URL afterwards.
  sync('topNav', () => {
    window.top.location.href = 'about:blank#pwned';
  });
}

void main();
