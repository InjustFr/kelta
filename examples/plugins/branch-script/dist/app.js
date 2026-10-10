// Branch script: finds the work item this screen was opened for, runs `./run.sh` on its branch and
// worktree, keeps the last 10 runs in `kv`. Everything goes through the permission-checked host API.
import { connect } from './kelta-sdk.js';

const $ = (id) => document.getElementById(id);
const MAX_RUNS = 10;
let item = null;
let running = false;

function text(el, value) {
  el.textContent = value;
}

// `params` comes from the action: `{ work = "{work.id}", ticket = "{ticket.key}" }`. A placeholder
// without a value expands to "", so a ticket without a work item has `work: ""`.
async function findWork(kelta, params) {
  if (params?.work) return kelta.work.get(params.work);
  if (params?.ticket) {
    const items = await kelta.work.list();
    return items.find((w) => w.ticket?.key === params.ticket) ?? null;
  }
  return null;
}

function renderWork(w) {
  const rows = [
    ['Branch', w.branch],
    ['Worktree', w.worktree],
    ['Ticket', w.ticket?.key ?? '—'],
    ['PR', w.pr_url ?? '—'],
  ];
  $('work').replaceChildren(
    ...rows.flatMap(([k, v]) => {
      const dt = document.createElement('dt');
      const dd = document.createElement('dd');
      dt.textContent = k;
      dd.textContent = v;
      return [dt, dd];
    }),
  );
}

async function renderRuns(kelta) {
  const runs = (await kelta.kv.get('runs')) ?? [];
  $('runs').replaceChildren(
    ...runs.map((r) => {
      const li = document.createElement('li');
      li.textContent = `${r.at} ${r.branch} → exit ${r.code}\n${r.output}`;
      return li;
    }),
  );
}

async function load(kelta) {
  try {
    item = await findWork(kelta, kelta.params);
  } catch (e) {
    item = null;
    text($('status'), `${e.code ?? 'error'}: ${e.message}`);
  }
  $('run').disabled = running || !item;
  if (!item) {
    text($('title'), 'No work item');
    $('work').replaceChildren();
    text($('status'), 'Open this screen from a work item or a ticket: "Run script on this branch".');
    return;
  }
  text($('title'), item.title ?? item.ticket?.key ?? item.branch);
  renderWork(item);
}

async function run(kelta) {
  // `load()` can replace `item` mid-run: keep the one this run started on.
  const w = item;
  if (running || !w) return;
  running = true;
  $('run').disabled = true;
  try {
    const { script = 'echo' } = await kelta.settings.get();
    text($('status'), `Running ${script}…`);
    const res = await kelta.exec('./run.sh', [script, w.branch, w.worktree], { cwd: w.worktree });
    text($('status'), `Exit ${res.code}`);
    // Keep the tail only: a kv value is capped at 64 KiB and exec output can reach 64 KiB per stream.
    const output = (res.stdout + res.stderr).slice(-2000);
    const runs = (await kelta.kv.get('runs')) ?? [];
    runs.unshift({ at: new Date().toLocaleString(), branch: w.branch, code: res.code, output });
    await kelta.kv.set('runs', runs.slice(0, MAX_RUNS));
    await renderRuns(kelta);
  } catch (e) {
    text($('status'), `${e.code ?? 'error'}: ${e.message}`);
  } finally {
    running = false;
    $('run').disabled = !item;
  }
}

async function main() {
  const kelta = await connect();
  await load(kelta);
  await renderRuns(kelta);
  $('run').addEventListener('click', () => void run(kelta));
  // The same screen can be reopened for another subject: the host pushes the new params.
  kelta.onParams(() => void load(kelta));
  // A step finished, the PR got merged: show the item as it is now.
  kelta.events.on('work.updated', (payload) => {
    if (payload?.work?.id === item?.id) void load(kelta);
  });
  kelta.events.on('pr.merged', () => void load(kelta));
  document.body.dataset.connected = 'true';
}

main().catch((e) => {
  text($('title'), `Could not connect to Kelta: ${e.message}`);
});
