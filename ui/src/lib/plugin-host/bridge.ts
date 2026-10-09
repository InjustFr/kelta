// Plugin screen host bridge (PLUGINS §7, ARCHITECTURE §11.3). The screen's SDK posts
// `{type: "kelta:ready"}` to the parent; only when that message comes from the iframe's own window
// does the host create a MessageChannel and transfer port2 with `kelta:init`. Every request on the
// port goes through `plugin_call`, where Rust checks the plugin's grants.

import type { JsonValue, PluginMethod, ScreenInstanceId } from '$lib/gen';
import { clipboardWrite, pluginCall } from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';

export const SCREEN_API = '0.1';

/** Theme tokens pushed to screens (a stable subset of tokens.css; plugins style against these). */
const THEME_TOKENS = [
  '--k-bg',
  '--k-bg-elev',
  '--k-bg-sunken',
  '--k-bg-hover',
  '--k-bg-active',
  '--k-bg-selected',
  '--k-fg',
  '--k-fg-muted',
  '--k-fg-subtle',
  '--k-border',
  '--k-border-strong',
  '--k-accent',
  '--k-accent-fg',
  '--k-danger',
  '--k-warn',
  '--k-ok',
  '--k-info',
  '--k-focus',
  '--k-font-ui',
  '--k-font-mono',
  '--k-font-size',
  '--k-radius',
];

/** Current theme as a CSS variable map (+ `color-scheme`). */
export function themeTokens(scheme: 'dark' | 'light'): Record<string, string> {
  const cs = getComputedStyle(document.documentElement);
  const out: Record<string, string> = { 'color-scheme': scheme };
  for (const name of THEME_TOKENS) {
    const v = cs.getPropertyValue(name).trim();
    if (v) out[name] = v;
  }
  return out;
}

/** Calls `cb` when the theme may have changed (`data-theme` on <html> or the OS preference). */
export function onThemeChange(cb: () => void): () => void {
  const mo = new MutationObserver(cb);
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'class'] });
  const mq = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
  mq?.addEventListener('change', cb);
  return () => {
    mo.disconnect();
    mq?.removeEventListener('change', cb);
  };
}

export interface ScreenInit {
  instance: ScreenInstanceId;
  plugin: string;
  project: string | null;
  params: JsonValue;
}

export type ScreenCall = (method: PluginMethod, params: JsonValue) => Promise<JsonValue>;

export interface ScreenBridge {
  /** Host push (`event`, `theme`, `params`, `visibility`); dropped until the screen connected. */
  push(message: { type: string } & Record<string, unknown>): void;
  destroy(): void;
}

/**
 * Serves one plugin iframe. `call` defaults to `plugin_call` for `init.instance`.
 * `theme` is read when the screen connects.
 */
export function connectScreen(
  iframe: HTMLIFrameElement,
  init: ScreenInit,
  theme: () => Record<string, string>,
  call: ScreenCall = (method, params) => pluginCall({ instance_id: init.instance, method, params }),
): ScreenBridge {
  let port: MessagePort | null = null;

  async function answer(port: MessagePort, data: unknown): Promise<void> {
    if (typeof data !== 'object' || data === null) return;
    const { id, method, params } = data as { id?: unknown; method?: unknown; params?: unknown };
    if (typeof id !== 'number' || typeof method !== 'string') return;
    try {
      const result = await call(method as PluginMethod, (params ?? {}) as JsonValue);
      // Rust only approves `clipboard.write` (grant check); the host performs the write.
      const ok = result as { approved?: unknown; text?: unknown } | null;
      if (method === 'clipboard.write' && ok?.approved === true && typeof ok.text === 'string') {
        await clipboardWrite({ kind: 'clipboard', text: ok.text });
      }
      port.postMessage({ id, result });
    } catch (err) {
      const e = toIpcError('plugin_call', err);
      port.postMessage({ id, error: { code: e.code, message: e.message, detail: e.detail } });
    }
  }

  function onMessage(ev: MessageEvent): void {
    const win = iframe.contentWindow;
    // A foreign window (another frame, an injected opener) never gets a port.
    if (!win || ev.source !== win) return;
    const data = ev.data as { type?: unknown } | null;
    if (data?.type !== 'kelta:ready') return;
    port?.close();
    const ch = new MessageChannel();
    const p = ch.port1;
    port = p;
    p.onmessage = (m) => void answer(p, m.data);
    win.postMessage({ type: 'kelta:init', api: SCREEN_API, ...init, theme: theme() }, '*', [ch.port2]);
  }

  window.addEventListener('message', onMessage);
  return {
    push(message) {
      port?.postMessage(message);
    },
    destroy() {
      window.removeEventListener('message', onMessage);
      port?.close();
      port = null;
    },
  };
}
