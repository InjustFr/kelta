// Layouts per project (ARCH §5.1). Local edits go through `update()`, which saves with
// `layout_save` 500 ms after the last change (one-shot timer, exists only while dirty) using the
// optimistic `rev`. A `Conflict` refetches the backend layout.

import type { KeltaError, Layout, OpenPaneRequest, ProjectId, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { isIpcError } from '$lib/ipc/transport';
import { openPane, type PaneLocation } from '$lib/layout';

import { toKeltaError } from './loadable';
import { reduceLayouts, type LayoutMap } from './reducers';

export const LAYOUT_SAVE_DEBOUNCE_MS = 500;

export class LayoutStore {
  byProject = $state<LayoutMap>({});
  errors = $state<Record<ProjectId, KeltaError | null>>({});
  /** Called when a save fails for another reason than a conflict (wired to toasts). */
  onSaveError: (err: unknown) => void = () => {};

  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive bookkeeping
  #timers = new Map<ProjectId, ReturnType<typeof setTimeout>>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive bookkeeping
  #saving = new Map<ProjectId, Promise<void>>();

  get(projectId: ProjectId): Layout | null {
    return this.byProject[projectId] ?? null;
  }

  isDirty(projectId: ProjectId): boolean {
    return this.#timers.has(projectId);
  }

  async load(projectId: ProjectId): Promise<Layout> {
    try {
      const layout = await ipc.layoutGet({ project_id: projectId });
      this.byProject = { ...this.byProject, [projectId]: layout };
      this.errors = { ...this.errors, [projectId]: null };
      return layout;
    } catch (err) {
      this.errors = { ...this.errors, [projectId]: toKeltaError(err, 'layout_get') };
      throw err;
    }
  }

  /** Loads the layout unless it is already in memory. */
  async ensure(projectId: ProjectId): Promise<Layout> {
    return this.byProject[projectId] ?? this.load(projectId);
  }

  /** Applies a pure layout op locally and schedules a save. */
  update(projectId: ProjectId, fn: (layout: Layout) => Layout): Layout | null {
    const current = this.byProject[projectId];
    if (!current) return null;
    const next = fn(current);
    if (next === current) return current;
    this.byProject = { ...this.byProject, [projectId]: next };
    this.#scheduleSave(projectId);
    return next;
  }

  /** Opens a pane per `OpenPaneRequest` placement in a loaded layout. */
  open(projectId: ProjectId, req: OpenPaneRequest): PaneLocation | null {
    let location: PaneLocation | null = null;
    this.update(projectId, (l) => {
      const r = openPane(l, req);
      location = r.location;
      return r.layout;
    });
    return location;
  }

  apply(ev: UiEvent): void {
    if (ev.type === 'layout.changed') {
      // Backend wins: drop any pending local save for this project.
      this.#cancel(ev.project_id);
    }
    const next = reduceLayouts(this.byProject, ev);
    if (next === this.byProject) return;
    this.byProject = next;
    if (ev.type === 'ui.open') this.#scheduleSave(ev.project_id);
  }

  /** Window close / unload: saves pending edits but never holds the close longer than `maxMs`. */
  flushForClose(maxMs = 1000): Promise<void> {
    return Promise.race([this.flush(), new Promise<void>((r) => setTimeout(r, maxMs))]);
  }

  /** Saves immediately (window close, tests). */
  async flush(projectId?: ProjectId): Promise<void> {
    const ids = projectId ? [projectId] : [...this.#timers.keys()];
    await Promise.all(
      ids.map((id) => {
        if (!this.#cancel(id)) return this.#saving.get(id) ?? Promise.resolve();
        return this.#save(id);
      }),
    );
  }

  #cancel(projectId: ProjectId): boolean {
    const t = this.#timers.get(projectId);
    if (t === undefined) return false;
    clearTimeout(t);
    this.#timers.delete(projectId);
    return true;
  }

  #scheduleSave(projectId: ProjectId): void {
    this.#cancel(projectId);
    // one-shot: debounced layout_save while the layout is dirty
    this.#timers.set(
      projectId,
      setTimeout(() => {
        this.#timers.delete(projectId);
        void this.#save(projectId);
      }, LAYOUT_SAVE_DEBOUNCE_MS),
    );
  }

  async #save(projectId: ProjectId): Promise<void> {
    const pending = this.#saving.get(projectId);
    if (pending) await pending;
    const layout = this.byProject[projectId];
    if (!layout) return;
    const run = (async () => {
      try {
        const { rev } = await ipc.layoutSave({ layout: $state.snapshot(layout) as Layout });
        const latest = this.byProject[projectId];
        if (latest) this.byProject = { ...this.byProject, [projectId]: { ...latest, rev } };
      } catch (err) {
        if (isIpcError(err, 'conflict')) {
          await this.load(projectId).catch(() => {});
        } else {
          this.onSaveError(err);
        }
      }
    })();
    this.#saving.set(projectId, run);
    try {
      await run;
    } finally {
      if (this.#saving.get(projectId) === run) this.#saving.delete(projectId);
    }
  }
}
