// Detached windows (#342): a second Kelta window showing one tab. Rust tells the page which one
// with `window.__KELTA_DETACHED__` (window/mod.rs `open_detached`); the main window has none.

import type { ProjectId, TabId } from '$lib/gen';

export interface DetachedTab {
  project_id: ProjectId;
  tab_id: TabId;
}

/** The tab this window shows, `null` in the main window. */
export const detached: DetachedTab | null =
  typeof window === 'undefined' ? null : (window.__KELTA_DETACHED__ ?? null);

/** Closes this detached window (its tab was reattached or closed). The mock has no native window. */
export function closeDetachedWindow(): void {
  if (import.meta.env.VITE_IPC === 'mock') return;
  void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => getCurrentWindow().destroy());
}
