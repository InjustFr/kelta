// kelta-bench in-app scenarios (ARCHITECTURE §13, gate G2). The bench sends `custom.bench.run`
// (forwarded only when KELTA_BENCH_MARKS is set) after its fixture setup; the scenario opens its own
// panes in the active project and reports each metric with `bench_mark`. The last key written is
// the one kelta-bench waits for.

import type { Placement } from '$lib/gen';
import { benchMark, perfSnapshot, sessionSpawn, sessionWrite } from '$lib/ipc/commands';
import { projects, sessions } from '$lib/stores';
import { frames, terminalPool, type TerminalView } from '$lib/terminal';
import { measureFps } from '$lib/terminal/raf';

import { openContent } from './nav';

const FRAME_MS = 1000 / 60;
/** Upper bound for waits (frames), well under kelta-bench's 60 s timeout. */
const WAIT_FRAMES = 1800;

/** Nearest-rank percentile. */
export function percentile(values: readonly number[], p: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.max(0, Math.ceil((p / 100) * sorted.length) - 1)] ?? NaN;
}

const nextFrame = (): Promise<number> =>
  new Promise((resolve) => frames.schedule(Symbol(), () => resolve(performance.now())));

async function until<T>(get: () => T | null | undefined, what: string): Promise<T> {
  for (let i = 0; i < WAIT_FRAMES; i++) {
    const v = get();
    if (v) return v;
    await nextFrame();
  }
  throw new Error(`bench: ${what} timed out`);
}

/** Opens `program` in a new pane of the active project; resolves with its attached view. */
async function openPane(program: string, args: string[], placement: Placement): Promise<TerminalView> {
  const projectId = await until(() => projects.activeId, 'active project');
  const created = await sessionSpawn({
    req: {
      id: null,
      project_id: projectId,
      kind: { type: 'shell' },
      name: `bench ${program}`,
      program,
      args,
      cwd: null,
      env: {},
      cols: 120,
      rows: 40,
      work_item_id: null,
      restore: { kind: 'none' },
      close_on_exit: 'never',
      template_id: null,
    },
  });
  sessions.upsert(created);
  await openContent(projectId, { content: { kind: 'terminal', session_id: created.id }, placement });
  return until(() => {
    const view = terminalPool.get<TerminalView>(created.id);
    return view?.state.attached && view.host.isConnected ? view : null;
  }, `${program} pane`);
}

/**
 * Writes one byte to `cat` (the tty echoes it) and times it until the frame that paints it.
 * shortcut: `writeMs` is the `session_write` round trip, an upper bound of keydown → PTY write;
 * upgrade to a Rust-side write timestamp if the keydown budget gets tight.
 */
async function echo(view: TerminalView): Promise<{ paintMs: number; writeMs: number }> {
  const parsed = new Promise<void>((resolve) => {
    const d = view.term.onWriteParsed(() => {
      d.dispose();
      resolve();
    });
  });
  const t0 = performance.now();
  await sessionWrite(view.id, 'x');
  const writeMs = performance.now() - t0;
  await parsed;
  return { paintMs: (await nextFrame()) - t0, writeMs };
}

/** Durations of the UI frames over `ms` (the first, partial one dropped). */
async function frameTimes(ms: number, onFrame?: () => void): Promise<number[]> {
  const times: number[] = [];
  let last = performance.now();
  await measureFps(ms, () => {
    const now = performance.now();
    times.push(now - last);
    last = now;
    onFrame?.();
  });
  return times.slice(1);
}

const webviewKb = async (): Promise<number> =>
  (await perfSnapshot()).processes
    .filter((p) => p.role !== 'core' && p.role !== 'child')
    .reduce((sum, p) => sum + p.pss_or_footprint_kb, 0);

async function echoLatency(): Promise<void> {
  const view = await openPane('cat', [], 'new_tab');
  const paint: number[] = [];
  const write: number[] = [];
  for (let i = 0; i < 100; i++) {
    const r = await echo(view);
    paint.push(r.paintMs);
    write.push(r.writeMs);
    await nextFrame();
  }
  await benchMark({ key: 'echo_frames_p50', value: percentile(paint, 50) / FRAME_MS });
  await benchMark({ key: 'echo_frames_p95', value: percentile(paint, 95) / FRAME_MS });
  await benchMark({ key: 'keydown_to_pty_p50_ms', value: percentile(write, 50) });
  await benchMark({ key: 'keydown_to_pty_p99_ms', value: percentile(write, 99) });
}

async function inkRedraw(): Promise<void> {
  await openPane('tui-sim', ['--ink', '--fps', '30'], 'new_tab');
  for (let i = 0; i < 60; i++) await nextFrame(); // let the redraw loop start
  await benchMark({ key: 'frame_p95_ms', value: percentile(await frameTimes(5000), 95) });
}

/** `tui-sim --flood 200` in one pane while `cat` echoes in the other. */
async function flood(): Promise<void> {
  const cat = await openPane('cat', [], 'new_tab');
  const before = await webviewKb();
  const flooder = await openPane('tui-sim', ['--flood', '200'], 'split_right');
  let inflight = 0;
  let echoMax = 0;
  let done = false;
  const measuring = frameTimes(10_000, () => (inflight = Math.max(inflight, flooder.inflight))).finally(
    () => (done = true),
  );
  while (!done) {
    echoMax = Math.max(echoMax, (await echo(cat)).paintMs);
    await nextFrame();
  }
  const times = await measuring;
  await benchMark({ key: 'frame_max_ms', value: Math.max(...times) });
  await benchMark({ key: 'echo_max_ms', value: echoMax });
  await benchMark({ key: 'webview_delta_mb', value: ((await webviewKb()) - before) / 1024 });
  await benchMark({ key: 'inflight_kib', value: inflight / 1024 });
}

const SCENARIOS: Record<string, () => Promise<void>> = {
  'echo-latency': echoLatency,
  'ink-redraw': inkRedraw,
  flood,
};

export async function runScenario(payload: unknown): Promise<void> {
  const name = (payload as { scenario?: string } | null)?.scenario ?? '';
  await SCENARIOS[name]?.();
}
