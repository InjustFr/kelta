// Gate S1 runner (BUILD_PLAN §6, ARCHITECTURE §11.3), run by scripts/sandbox-check.sh inside Docker:
//   node fixtures/sandbox/run.mjs <kelta binary>
// Starts the real app under tauri-driver/WebKitWebDriver with a throwaway HOME, opens the hostile
// sandbox-probe plugin screen and the same page as a localhost web tool, and reads what probe.js
// could do from inside each iframe. Exit 0 only when every escape attempt failed.
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const bin = process.argv[2];
if (!bin) throw new Error('usage: run.mjs <kelta binary>');
const probeDir = join(dirname(fileURLToPath(import.meta.url)), 'sandbox-probe');
const PLUGIN = 'sandbox-probe';
// Declared but not granted: tickets.read (probe.js tries to grant it to itself).
const GRANTED = ['projects.read', 'ui.open'];

// The web tool's loopback server: same page as the plugin screen.
const server = createServer((req, res) => {
  const file = req.url.startsWith('/probe.js') ? 'probe.js' : 'index.html';
  res.setHeader('Content-Type', file.endsWith('.js') ? 'text/javascript' : 'text/html; charset=utf-8');
  res.end(req.method === 'HEAD' ? undefined : readFileSync(join(probeDir, file)));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const toolUrl = `http://127.0.0.1:${server.address().port}/index.html`;

// Throwaway HOME: never the user's real config or data.
const home = mkdtempSync(join(tmpdir(), 'kelta-s1-'));
const env = {
  ...process.env,
  HOME: home,
  XDG_CONFIG_HOME: join(home, 'config'),
  XDG_DATA_HOME: join(home, 'data'),
  XDG_STATE_HOME: join(home, 'state'),
  XDG_RUNTIME_DIR: join(home, 'run'),
};
mkdirSync(join(home, 'config', 'kelta'), { recursive: true });
mkdirSync(join(home, 'run'), { recursive: true, mode: 0o700 });
writeFileSync(
  join(home, 'config', 'kelta', 'config.toml'),
  `[plugins]
dev_paths = [${JSON.stringify(probeDir)}]

[[tools]]
id = "s1-probe"
label = "S1 probe"
kind = "web"
url = ${JSON.stringify(toolUrl)}
embed = "iframe"
`,
);

const driver = spawn('tauri-driver', ['--port', '4444'], { env, stdio: ['ignore', 'inherit', 'pipe'] });
// The app's stderr comes through the driver: collected to prove the forged IPC reached the invoke-key check.
let stderr = '';
driver.stderr.on('data', (d) => {
  process.stderr.write(d);
  stderr += d;
});
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const failures = [];
let session;

async function wd(method, path, body) {
  const r = await fetch(`http://127.0.0.1:4444${path}`, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await r.json();
  if (!r.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json.value)}`);
  return json.value;
}
const s = (path) => `/session/${session}${path}`;

async function until(what, fn, ms = 30000) {
  const end = Date.now() + ms;
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {
      if (Date.now() > end) throw new Error(`${what}: ${e.message}`);
    }
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(250);
  }
}

/** Invokes a Kelta command from the main frame (the trusted app page). */
async function invoke(cmd, args) {
  const r = await wd('POST', s('/execute/async'), {
    script: `const [cmd, args, done] = arguments;
      window.__TAURI_INTERNALS__.invoke(cmd, args).then((ok) => done({ ok }), (err) => done({ err }));`,
    args: [cmd, args],
  });
  if ('err' in r) throw new Error(`${cmd}: ${JSON.stringify(r.err)}`);
  return r.ok;
}

function check(name, ok, detail) {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail === undefined ? '' : `: ${detail}`}`);
  if (!ok) failures.push(name);
}

/** Switches into the iframe matching `selector`, waits for probe.js and returns its results. */
async function probe(selector) {
  const el = await until(selector, () => wd('POST', s('/element'), { using: 'css selector', value: selector }));
  await wd('POST', s('/frame'), { id: el });
  const body = await until('probe results', () => wd('POST', s('/element'), { using: 'css selector', value: 'body[data-s1]' }));
  const ref = Object.values(body)[0];
  const results = JSON.parse(await wd('GET', s(`/element/${ref}/attribute/data-s1`)));
  await wd('POST', s('/frame'), { id: null });
  return results;
}

const blocked = (v) => v === 'undefined' || v.startsWith('threw');
const ctx = { repo_id: null, cwd: null, session_id: null, work_item_id: null, ticket: null, review: null, extra: {} };

function assertProbe(kind, r, bridge) {
  console.log(`\n${kind}: ${JSON.stringify(r)}`);
  check(`${kind}: no __TAURI_INTERNALS__`, r.internals === 'undefined', r.internals);
  check(`${kind}: no __TAURI__`, r.tauri === 'undefined', r.tauri);
  check(`${kind}: parent __TAURI_INTERNALS__ unreachable`, blocked(r.parentInternals), r.parentInternals);
  check(`${kind}: top __TAURI_INTERNALS__ unreachable`, blocked(r.topInternals), r.topInternals);
  check(`${kind}: parent DOM unreadable`, r.parentRead.startsWith('threw'), r.parentRead);
  check(`${kind}: parent DOM unwritable`, r.parentWrite.startsWith('threw'), r.parentWrite);
  check(`${kind}: top DOM unwritable`, r.topWrite.startsWith('threw'), r.topWrite);
  for (const k of ['fetchIpcScheme', 'fetchIpcHttp'])
    check(`${kind}: ${k} rejected`, !/^status 2/.test(r[k]), r[k]);
  check(`${kind}: bridge ${bridge.granted}`, r.granted === bridge.granted, r.granted);
  check(`${kind}: bridge ${bridge.notGranted}`, r.notGranted === bridge.notGranted, r.notGranted);
}

/** After each iframe: the top window was not navigated or written, grants unchanged, IPC still works. */
async function assertApp(kind) {
  await sleep(500); // let a forged IPC message or a top navigation land before looking
  const url = await wd('GET', s('/url'));
  check(`${kind}: top window not navigated`, url.startsWith('tauri://localhost'), url);
  const pwned = await wd('POST', s('/execute/sync'), { script: 'return document.body.dataset.pwned ?? null', args: [] });
  check(`${kind}: app DOM untouched`, pwned === null, pwned);
  const plugin = (await invoke('plugin_list', {})).find((p) => p.id === PLUGIN);
  const grants = JSON.stringify(plugin?.granted.toSorted());
  check(`${kind}: grants unchanged`, grants === JSON.stringify(GRANTED), grants);
}

try {
  await until('tauri-driver', () => fetch('http://127.0.0.1:4444/status').then((r) => r.ok), 15000);
  session = (
    await wd('POST', '/session', { capabilities: { alwaysMatch: { 'tauri:options': { application: bin } } } })
  ).sessionId;
  await until('Kelta IPC in the main frame', () =>
    wd('POST', s('/execute/sync'), { script: 'return !!window.__TAURI_INTERNALS__', args: [] }),
  );
  check('main frame has __TAURI_INTERNALS__ (control)', true);

  await invoke('plugin_grant', { id: PLUGIN, permissions: GRANTED });
  await invoke('command_run', { command_id: `${PLUGIN}.open`, ctx });
  const screen = await probe(`iframe[src^="kelta-plugin://${PLUGIN}/"]`);
  assertProbe('plugin screen', screen, { granted: 'ok', notGranted: 'permission_denied' });
  await assertApp('plugin screen');

  await invoke('tool_open', { project_id: 'home', tool_id: 's1-probe', ctx, placement: 'new_tab' });
  const tool = await probe(`iframe[src="${toolUrl}"]`);
  assertProbe('web tool', tool, { granted: 'no bridge', notGranted: 'no bridge' });
  await assertApp('web tool');
  // One forged message per iframe (probe.js webkitIpc); "grants unchanged" alone also passes if Tauri drops it earlier.
  check('forged IPC hit the invoke-key check', (stderr.match(/__TAURI_INVOKE_KEY__ expected/g) ?? []).length >= 2);
} catch (e) {
  check('runner', false, e.stack);
} finally {
  if (session) await wd('DELETE', s('')).catch(() => {});
  driver.kill();
  server.close();
}

console.log(failures.length ? `\nS1 FAILED: ${failures.length} check(s)` : '\nS1 passed');
process.exit(failures.length ? 1 : 0);
