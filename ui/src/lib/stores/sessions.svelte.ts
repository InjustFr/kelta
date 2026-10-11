// Sessions across all projects. Fed by session.* UiEvents.

import type { ClaudeRun, KeltaError, ProjectId, SessionId, SessionInfo, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { toKeltaError } from './loadable';
import { nextNeedingInput, reduceSessions, sessionsToMap, type SessionMap } from './reducers';

export class SessionsStore {
  byId = $state<SessionMap>({});
  loaded = $state(false);
  error = $state<KeltaError | null>(null);

  get all(): SessionInfo[] {
    return Object.values(this.byId);
  }

  get(id: SessionId): SessionInfo | null {
    return this.byId[id] ?? null;
  }

  forProject(projectId: ProjectId): SessionInfo[] {
    return this.all.filter((s) => s.project_id === projectId);
  }

  get needingInput(): SessionInfo[] {
    return this.all.filter((s) => s.attention === 'needs_input' || s.status === 'needs_input');
  }

  /** True when Claude (or any session) in the project is busy: feeds the `working` lamp. */
  workingIn(projectId: ProjectId): boolean {
    return this.all.some((s) => s.project_id === projectId && s.status === 'working');
  }

  next(projectOrder: readonly ProjectId[], afterId: SessionId | null): SessionInfo | null {
    return nextNeedingInput(this.all, projectOrder, afterId);
  }

  /** Claude `Workflow` runs per session, in first-seen order (#272). */
  runs = $state<Record<SessionId, ClaudeRun[]>>({});
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- a load guard, never rendered
  #runsLoaded = new Set<SessionId>();

  apply(ev: UiEvent): void {
    const next = reduceSessions(this.byId, ev);
    if (next !== this.byId) this.byId = next;
    if (ev.type === 'workflow.updated') this.putRun(ev.run);
    else if (ev.type === 'session.removed' && ev.id in this.runs) {
      const { [ev.id]: _gone, ...rest } = this.runs;
      this.runs = rest;
    }
  }

  /** Once per session: the runs core saw before this window listened (events keep them current). */
  async loadRuns(id: SessionId): Promise<void> {
    if (this.#runsLoaded.has(id)) return;
    this.#runsLoaded.add(id);
    try {
      for (const run of await ipc.claudeRunsList({ id })) this.putRun(run);
    } catch {
      this.#runsLoaded.delete(id);
    }
  }

  private putRun(run: ClaudeRun): void {
    const list = this.runs[run.session_id] ?? [];
    const i = list.findIndex((r) => r.run_id === run.run_id);
    const next = i < 0 ? [...list, run] : list.map((r, j) => (j === i ? run : r));
    this.runs = { ...this.runs, [run.session_id]: next };
  }

  /** Loads every session (or one project's, merged into the map). */
  async load(projectId?: ProjectId): Promise<void> {
    try {
      const list = await ipc.sessionList({ project_id: projectId ?? null });
      if (projectId) {
        const kept: SessionMap = {};
        for (const [id, s] of Object.entries(this.byId)) if (s.project_id !== projectId) kept[id] = s;
        this.byId = { ...kept, ...sessionsToMap(list) };
      } else {
        this.byId = sessionsToMap(list);
      }
      this.error = null;
      this.loaded = true;
    } catch (err) {
      this.error = toKeltaError(err, 'session_list');
      throw err;
    }
  }

  /** Records a session returned by a command (spawn, rename, restart…). */
  upsert(session: SessionInfo): SessionInfo {
    this.apply({ type: 'session.updated', session });
    return session;
  }

  async rename(id: SessionId, name: string): Promise<SessionInfo> {
    return this.upsert(await ipc.sessionRename({ id, name }));
  }

  async restart(id: SessionId): Promise<SessionInfo> {
    return this.upsert(await ipc.sessionRestart({ id }));
  }

  async kill(id: SessionId, force = false): Promise<void> {
    await ipc.sessionKill({ id, force });
  }

  /** Clears attention locally right away; the backend confirms with session.updated. */
  async markSeen(id: SessionId): Promise<void> {
    const s = this.byId[id];
    if (s && !s.seen) this.byId = { ...this.byId, [id]: { ...s, seen: true } };
    await ipc.sessionMarkSeen({ id });
  }
}
