// The one project scope of the global lists, Now, Tickets and Reviews (#409): all projects or one.
// Set from the window's top bar (#429), kept in the webview's storage like the rail.

import type { ProjectId, Scope } from '$lib/gen';
import { layout, projects } from '$lib/stores';

const KEY = 'kelta.scope.v1';

function saved(): ProjectId | null {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null') as unknown;
    return typeof v === 'string' ? v : null;
  } catch {
    return null;
  }
}

export class ListScope {
  #project = $state<ProjectId | null>(saved());

  /** The project the lists show; `null` = all projects, also for Home or a project closed since. */
  get project(): ProjectId | null {
    const id = this.#project;
    if (!id || !projects.loaded) return id;
    const p = projects.byId(id);
    return p && p.open && !p.builtin ? id : null;
  }

  get scope(): Scope {
    const id = this.project;
    return id ? { kind: 'project', id } : { kind: 'all' };
  }

  set(id: ProjectId | null): void {
    this.#project = id;
    try {
      localStorage.setItem(KEY, JSON.stringify(id));
    } catch {
      // storage unavailable: the scope lives for this window only
    }
  }
}

export const listScope = new ListScope();

// A project's Board, Reviews or Inbox tab closed on load (#409): its project becomes the scope, once.
layout.onListTabsClosed = (s) => listScope.set(s.kind === 'project' ? s.id : null);
