// Sessions across all projects. Fed by session.* UiEvents.

import type { KeltaError, ProjectId, SessionId, SessionInfo, UiEvent } from '$lib/gen';
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

  next(projectOrder: readonly ProjectId[], afterId: SessionId | null): SessionInfo | null {
    return nextNeedingInput(this.all, projectOrder, afterId);
  }

  apply(ev: UiEvent): void {
    const next = reduceSessions(this.byId, ev);
    if (next !== this.byId) this.byId = next;
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
