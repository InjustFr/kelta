// Linux first-run renderer probe (ARCHITECTURE §9.2): a 2 s scroll test per renderer. When WebGL
// renders ≥ 55 fps while the DOM renderer stays under 45 fps, a toast offers switching. The result
// is remembered so the probe runs once; it is stored in the webview's localStorage because the
// contract has no command for the SQLite app state (see docs/contract-requests/L2.md).

import { Terminal } from '@xterm/xterm';

import { measureFps } from './raf';
import { probeVerdict, webglSupported } from './renderer';
import { xtermTheme } from './theme';

const STORAGE_KEY = 'kelta.render_probe.v1';
const DURATION_MS = 2000;

export interface ProbeResult {
  webglFps: number;
  domFps: number;
  verdict: ReturnType<typeof probeVerdict>;
}

export function probeDone(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) !== null;
  } catch {
    return false;
  }
}

function remember(result: ProbeResult): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(result));
  } catch {
    // storage unavailable: the probe may run again next start
  }
}

async function scrollTest(webgl: boolean): Promise<number> {
  const host = document.createElement('div');
  host.style.cssText = 'position:fixed;left:-10000px;top:0;width:900px;height:420px;';
  document.body.appendChild(host);
  const term = new Terminal({ cols: 110, rows: 24, allowProposedApi: true, theme: xtermTheme('dark') });
  try {
    term.open(host);
    if (webgl) {
      const { WebglAddon } = await import('@xterm/addon-webgl');
      term.loadAddon(new WebglAddon());
    }
    const line = '\x1b[32mscroll\x1b[0m the quick brown fox jumps over the lazy dog 0123456789 '.repeat(2);
    return await measureFps(DURATION_MS, (i) => {
      term.write(`${i} ${line}\r\n${i} ${line}\r\n${i} ${line}\r\n`);
    });
  } finally {
    term.dispose();
    host.remove();
  }
}

export interface ProbeOptions {
  platform: 'macos' | 'linux';
  /** `terminal.renderer`: only `auto` is probed. */
  renderer: string;
  onSuggest: (result: ProbeResult) => void;
}

let started = false;

/** Runs the probe once per install, on Linux with `renderer = auto`, outside automation. */
export async function maybeRunRenderProbe(options: ProbeOptions): Promise<ProbeResult | null> {
  if (started || probeDone()) return null;
  if (options.platform !== 'linux' || options.renderer !== 'auto') return null;
  if (typeof navigator !== 'undefined' && navigator.webdriver) return null;
  if (document.hidden || !webglSupported()) return null;
  started = true;
  try {
    const domFps = await scrollTest(false);
    const webglFps = await scrollTest(true);
    const result: ProbeResult = { webglFps, domFps, verdict: probeVerdict(webglFps, domFps) };
    remember(result);
    if (result.verdict === 'suggest-webgl') options.onSuggest(result);
    return result;
  } catch {
    return null;
  }
}
