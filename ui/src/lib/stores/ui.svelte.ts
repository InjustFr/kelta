// Overlays and sheets (palette, switcher, sheet stack) and ctl.command routing.

import type { CtlCommand, UiEvent } from '$lib/gen';

export type SheetKey =
  'start_work' | 'onboarding' | 'project_new' | 'plugin_install' | 'tool_picker' | 'session_new';
export type OverlayKey = 'palette' | 'switcher';

export interface SheetEntry {
  key: SheetKey;
  props: Record<string, unknown>;
}

export class UiStore {
  overlay = $state<OverlayKey | null>(null);
  /** Initial query for the palette (e.g. "Move SHOP-142 to…"). */
  paletteQuery = $state('');
  sheets = $state<SheetEntry[]>([]);
  /** Window visibility/focus as seen by the webview. */
  focused = $state(true);
  /** Inbox pseudo-project selected on the rail. */
  inboxActive = $state(false);

  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive listener list
  #ctlListeners = new Set<(cmd: CtlCommand) => void>();

  get sheet(): SheetEntry | null {
    return this.sheets[this.sheets.length - 1] ?? null;
  }

  openOverlay(key: OverlayKey, query = ''): void {
    this.overlay = key;
    this.paletteQuery = query;
  }

  closeOverlay(): void {
    this.overlay = null;
    this.paletteQuery = '';
  }

  toggleOverlay(key: OverlayKey): void {
    if (this.overlay === key) this.closeOverlay();
    else this.openOverlay(key);
  }

  openSheet(key: SheetKey, props: Record<string, unknown> = {}): void {
    this.sheets = [...this.sheets, { key, props }];
  }

  closeSheet(key?: SheetKey): void {
    if (!key) {
      this.sheets = this.sheets.slice(0, -1);
      return;
    }
    const i = this.sheets.map((s) => s.key).lastIndexOf(key);
    if (i >= 0) this.sheets = this.sheets.filter((_, j) => j !== i);
  }

  /** Listens to ctl.command events (kelta-ctl, second instance). Returns an unsubscribe fn. */
  onCtl(listener: (cmd: CtlCommand) => void): () => void {
    this.#ctlListeners.add(listener);
    return () => this.#ctlListeners.delete(listener);
  }

  apply(ev: UiEvent): void {
    if (ev.type !== 'ctl.command') return;
    if (ev.cmd.cmd === 'palette') this.openOverlay('palette');
    for (const l of [...this.#ctlListeners]) l(ev.cmd);
  }
}
