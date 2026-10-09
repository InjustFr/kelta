// Work items. Fed by work.updated UiEvents (start-work progress, PR, finish).

import type {
  GitStatus,
  KeltaError,
  ProjectId,
  SessionId,
  TicketRef,
  UiEvent,
  WorkItem,
  WorkItemId,
} from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { toKeltaError } from './loadable';
import { reduceWork, type WorkMap } from './reducers';

export class WorkStore {
  byId = $state<WorkMap>({});
  loaded = $state(false);
  error = $state<KeltaError | null>(null);
  /** Git status of every unfinished item (`work_status_all`), with the time it was read. */
  git = $state<Record<WorkItemId, GitStatus>>({});
  gitAt = $state<number | null>(null);
  gitError = $state(false);
  #gitPending: Promise<void> | null = null;
  #gitAgain = false;

  get all(): WorkItem[] {
    return Object.values(this.byId);
  }

  get(id: WorkItemId): WorkItem | null {
    return this.byId[id] ?? null;
  }

  forProject(projectId: ProjectId): WorkItem[] {
    return this.all.filter((w) => w.project_id === projectId);
  }

  /** Unfinished work item for a ticket, if any. */
  forTicket(ref: TicketRef): WorkItem | null {
    return (
      this.all.find(
        (w) =>
          w.ticket &&
          w.ticket.account === ref.account &&
          w.ticket.key === ref.key &&
          w.state.kind !== 'finished',
      ) ?? null
    );
  }

  forSession(sessionId: SessionId): WorkItem | null {
    return this.all.find((w) => w.session_ids.includes(sessionId)) ?? null;
  }

  /**
   * `work_status_all` (startup, window focus, Now open, a Claude stop). A request made while one is
   * in flight runs once more after it, so the result is never older than the request.
   */
  refreshStatus(): Promise<void> {
    if (this.#gitPending) {
      this.#gitAgain = true;
      return this.#gitPending;
    }
    this.#gitPending = ipc
      .workStatusAll({})
      .then((all) => {
        this.git = all;
        this.gitAt = Date.now();
        this.gitError = false;
      })
      .catch(() => {
        this.gitError = true;
      })
      .finally(() => {
        this.#gitPending = null;
        if (this.#gitAgain) {
          this.#gitAgain = false;
          void this.refreshStatus();
        }
      });
    return this.#gitPending;
  }

  apply(ev: UiEvent): void {
    const next = reduceWork(this.byId, ev);
    if (next !== this.byId) this.byId = next;
  }

  upsert(item: WorkItem): WorkItem {
    this.apply({ type: 'work.updated', work: item });
    return item;
  }

  async load(projectId?: ProjectId): Promise<void> {
    try {
      const list = await ipc.workList({ project_id: projectId ?? null });
      const next: WorkMap = projectId
        ? Object.fromEntries(Object.entries(this.byId).filter(([, w]) => w.project_id !== projectId))
        : {};
      for (const w of list) next[w.id] = w;
      this.byId = next;
      this.error = null;
      this.loaded = true;
    } catch (err) {
      this.error = toKeltaError(err, 'work_list');
      throw err;
    }
  }
}
