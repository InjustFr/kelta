// Overlays and sheets (palette, switcher, sheet stack) and ctl.command routing.

import type { CtlCommand, UiEvent } from '$lib/gen';

export type SheetKey =
  | 'start_work'
  | 'start_batch'
  | 'work_new'
  | 'link_ticket'
  | 'create_ticket'
  | 'onboarding'
  | 'project_new'
  | 'plugin_install'
  | 'tool_picker'
  | 'session_new'
  | 'ship'
  | 'finish'
  | 'finish_merged'
  | 'merge'
  | 'fix'
  | 'work_dialog'
  | 'tracker.source_picker'
  | 'tickets.move'
  | 'workflow_run'
  | 'claude_runs'
  | 'groom';
export type OverlayKey = 'palette' | 'switcher';
/** The cross-project places on the rail (SPEC §1), shown outside any project. */
export type GlobalView = 'tickets' | 'now' | 'reviews';
/** The global views' built-in layout (a tab per view) and `projectId` of their panes: never a real
 *  project, never opened into. */
export const GLOBAL = 'global';

export interface SheetEntry {
  key: SheetKey;
  props: Record<string, unknown>;
}

export class UiStore {
  overlay = $state<OverlayKey | null>(null);
  /** Initial query for the palette (e.g. "Move SHOP-142 to…"). */
  paletteQuery = $state('');
  sheets = $state<SheetEntry[]>([]);
  /** The switcher runs as `pickProject`'s picker (its placeholder and pending answer) while set. */
  pick = $state<{ placeholder: string; resolve: (id: string | null) => void } | null>(null);
  /** Window visibility/focus as seen by the webview. */
  focused = $state(true);
  /** Global view shown instead of the active project (`null` = the project). */
  view = $state<GlobalView | null>(null);
  /** A cross link's target in a global view (row key), read once by that view's pane on mount, then cleared. */
  select = $state<{ view: GlobalView; key: string } | null>(null);

  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive listener list
  #ctlListeners = new Set<(cmd: CtlCommand) => void>();

  get sheet(): SheetEntry | null {
    return this.sheets[this.sheets.length - 1] ?? null;
  }

  openOverlay(key: OverlayKey, query = ''): void {
    this.#settlePick(null);
    this.overlay = key;
    this.paletteQuery = query;
  }

  closeOverlay(): void {
    this.#settlePick(null);
    this.overlay = null;
    this.paletteQuery = '';
  }

  /** The switcher as a project picker (no Home): the picked project id, or null when closed. */
  pickProject(placeholder: string): Promise<string | null> {
    this.openOverlay('switcher');
    return new Promise((resolve) => (this.pick = { placeholder, resolve }));
  }

  /** Answer of the open `pickProject` picker. */
  resolvePick(id: string): void {
    this.#settlePick(id);
    this.closeOverlay();
  }

  #settlePick(id: string | null): void {
    const p = this.pick;
    this.pick = null;
    p?.resolve(id);
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
    else if (ev.cmd.cmd === 'plugin_install') this.openSheet('plugin_install', { source: ev.cmd.source });
    for (const l of [...this.#ctlListeners]) l(ev.cmd);
  }
}
