// Store singletons, the UiEvent dispatcher and startup loading.

import type { UiEvent } from '$lib/gen';
import { connectUiEvents, onAnyUiEvent } from '$lib/ipc/events';

import { AttentionStore } from './attention.svelte';
import { LayoutStore } from './layout.svelte';
import { PluginsStore } from './plugins.svelte';
import { ProjectsStore } from './projects.svelte';
import { ReviewsStore } from './reviews.svelte';
import { SessionsStore } from './sessions.svelte';
import { SettingsStore } from './settings.svelte';
import { TicketsStore } from './tickets.svelte';
import { ToastsStore } from './toasts.svelte';
import { ToolsStore } from './tools.svelte';
import { UiStore } from './ui.svelte';
import { WorkStore } from './work.svelte';

export { AttentionStore, LayoutStore, PluginsStore, ProjectsStore, ReviewsStore, SessionsStore };
export { SettingsStore, TicketsStore, ToastsStore, ToolsStore, UiStore, WorkStore };
export type { Loadable } from './loadable';
export * from './reducers';

export const projects = new ProjectsStore();
export const sessions = new SessionsStore();
export const layout = new LayoutStore();
export const tickets = new TicketsStore();
export const reviews = new ReviewsStore();
export const work = new WorkStore();
export const settings = new SettingsStore();
export const attention = new AttentionStore();
export const toasts = new ToastsStore();
export const ui = new UiStore();
export const tools = new ToolsStore();
export const plugins = new PluginsStore();

layout.onSaveError = (err) => toasts.error(err, 'Saving layout failed');

const ALL = [
  projects,
  sessions,
  layout,
  tickets,
  reviews,
  work,
  settings,
  attention,
  toasts,
  ui,
  tools,
  plugins,
];

/** Routes one UiEvent to every store (each ignores the types it does not handle). */
export function applyUiEvent(ev: UiEvent): void {
  for (const store of ALL) {
    try {
      store.apply(ev);
    } catch (err) {
      console.error(`[kelta] store failed on ${ev.type}`, err);
    }
  }
}

let started: Promise<BootstrapResult> | null = null;

export interface BootstrapResult {
  /** Commands that failed during startup (the stub backend fails most of them). */
  failed: string[];
}

/**
 * Subscribes to UiEvents and loads the startup data: projects, sessions, work items, settings and
 * the active project's layout. Never throws: failures are reported once as a toast.
 */
export function bootstrap(): Promise<BootstrapResult> {
  if (started) return started;
  started = (async () => {
    onAnyUiEvent(applyUiEvent);
    const tasks: [string, Promise<unknown>][] = [
      ['events_subscribe', connectUiEvents()],
      [
        'project_list',
        projects.load().then(() => {
          attention.seed(projects.list);
          // Per-project overrides (e.g. claude.budget_usd) are read via settings.value(projectId).
          for (const p of projects.list) void settings.load(p.id);
        }),
      ],
      ['session_list', sessions.load()],
      ['work_list', work.load()],
      ['settings_effective', settings.load().then((r) => (r.error ? Promise.reject(r.error) : r))],
    ];
    const results = await Promise.allSettled(tasks.map(([, p]) => p));
    const failed = tasks.filter((_, i) => results[i]?.status === 'rejected').map(([name]) => name);
    const active = projects.active;
    if (active) {
      await layout.load(active.id).catch(() => failed.push('layout_get'));
    }
    if (failed.length > 0) {
      toasts.push({ level: 'warn', text: `Backend not ready: ${failed.join(', ')} failed`, action: null });
    }
    return { failed };
  })();
  return started;
}
