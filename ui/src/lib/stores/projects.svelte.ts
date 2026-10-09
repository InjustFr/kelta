// Projects (rail order = list order). Fed by project.* and attention.changed UiEvents.

import type { KeltaError, ProjectId, ProjectInfo, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { toKeltaError } from './loadable';
import { reduceProjects } from './reducers';

export class ProjectsStore {
  list = $state<ProjectInfo[]>([]);
  loaded = $state(false);
  loading = $state(false);
  error = $state<KeltaError | null>(null);

  get active(): ProjectInfo | null {
    return this.list.find((p) => p.active) ?? null;
  }

  get activeId(): ProjectId | null {
    return this.active?.id ?? null;
  }

  /** Projects shown on the rail (open), in order. */
  get openProjects(): ProjectInfo[] {
    return this.list.filter((p) => p.open);
  }

  get home(): ProjectInfo | null {
    return this.list.find((p) => p.builtin) ?? null;
  }

  byId(id: ProjectId): ProjectInfo | null {
    return this.list.find((p) => p.id === id) ?? null;
  }

  apply(ev: UiEvent): void {
    const next = reduceProjects(this.list, ev);
    if (next !== this.list) this.list = next;
  }

  async load(): Promise<void> {
    this.loading = true;
    try {
      this.list = await ipc.projectList();
      this.error = null;
      this.loaded = true;
    } catch (err) {
      this.error = toKeltaError(err, 'project_list');
      throw err;
    } finally {
      this.loading = false;
    }
  }

  #upsert(p: ProjectInfo): ProjectInfo {
    this.apply({ type: 'project.updated', project: p });
    return p;
  }

  /** Optimistically marks `id` active, then confirms with the backend (rolls back on error). */
  async activate(id: ProjectId): Promise<ProjectInfo> {
    const before = this.list;
    this.list = this.list.map((p) => ({ ...p, active: p.id === id, open: p.id === id ? true : p.open }));
    try {
      return this.#upsert(await ipc.projectActivate({ id }));
    } catch (err) {
      this.list = before;
      throw err;
    }
  }

  async open(id: ProjectId): Promise<ProjectInfo> {
    return this.#upsert(await ipc.projectOpen({ id }));
  }

  async close(id: ProjectId, killSessions = false): Promise<ProjectInfo> {
    return this.#upsert(await ipc.projectClose({ id, kill_sessions: killSessions }));
  }

  async remove(id: ProjectId, killSessions = false): Promise<void> {
    await ipc.projectRemove({ id, kill_sessions: killSessions });
    this.apply({ type: 'project.removed', id });
  }

  /** Reorders locally, then persists. Rolls back on error. */
  async reorder(ids: ProjectId[]): Promise<void> {
    const before = this.list;
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local lookup table
    const order = new Map(ids.map((id, i) => [id, i]));
    this.list = [...this.list].sort((a, b) => (order.get(a.id) ?? 1e9) - (order.get(b.id) ?? 1e9));
    try {
      await ipc.projectReorder({ ids });
    } catch (err) {
      this.list = before;
      throw err;
    }
  }
}
