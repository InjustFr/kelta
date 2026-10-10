// Pure reducers: (state, UiEvent) → state. No runes, no IPC, so they are trivially unit-tested.
// The $state stores in this directory delegate every UiEvent to these functions.

import type {
  AccountId,
  AccountStatus,
  Attention,
  Layout,
  ProjectId,
  ProjectInfo,
  ReviewRef,
  Scope,
  SessionId,
  SessionInfo,
  UiEvent,
  WorkItem,
  WorkItemId,
} from '$lib/gen';
import { openPane } from '$lib/layout';

// ---- helpers --------------------------------------------------------------------------------

export const ATTENTION_ORDER: readonly Attention[] = ['none', 'activity', 'done', 'error', 'needs_input'];

export function attentionRank(a: Attention): number {
  return ATTENTION_ORDER.indexOf(a);
}

/** Max-aggregation of attention levels (ARCH §5: ordered enum). */
export function maxAttention(levels: Iterable<Attention>): Attention {
  let best: Attention = 'none';
  for (const l of levels) if (attentionRank(l) > attentionRank(best)) best = l;
  return best;
}

/** What a lamp shows: an attention level, or `working` derived from session status (not an Attention). */
export type LampLevel = Attention | 'working';

/** Precedence needs_input > error > working > done > activity (DESIGN §6.1). */
export function lampOf(level: Attention, working: boolean): LampLevel {
  return working && level !== 'needs_input' && level !== 'error' ? 'working' : level;
}

/** Stable key for a `Scope` (`all` or `project:<id>`). */
export function scopeKey(scope: Scope): string {
  return scope.kind === 'all' ? 'all' : `project:${scope.id}`;
}

/** Whether data loaded for `loaded` is affected by a change announced for `changed`. */
export function scopeAffects(changed: Scope, loaded: Scope): boolean {
  if (changed.kind === 'all' || loaded.kind === 'all') return true;
  return changed.id === loaded.id;
}

export function reviewKey(r: ReviewRef): string {
  return `${r.account}:${r.repo}#${r.number}`;
}

// ---- projects -------------------------------------------------------------------------------

export function reduceProjects(list: readonly ProjectInfo[], ev: UiEvent): ProjectInfo[] {
  switch (ev.type) {
    case 'project.updated': {
      const index = list.findIndex((p) => p.id === ev.project.id);
      let next = index < 0 ? [...list, ev.project] : list.map((p, i) => (i === index ? ev.project : p));
      // A single project is active at a time.
      if (ev.project.active)
        next = next.map((p) => (p.id !== ev.project.id && p.active ? { ...p, active: false } : p));
      return next;
    }
    case 'project.removed':
      return list.filter((p) => p.id !== ev.id);
    case 'attention.changed':
      return list.map((p) =>
        p.id === ev.project_id
          ? { ...p, attention: { level: ev.level, needs_input: ev.needs_input_count } }
          : p,
      );
    default:
      return list as ProjectInfo[];
  }
}

// ---- sessions -------------------------------------------------------------------------------

export type SessionMap = Record<SessionId, SessionInfo>;

export function reduceSessions(map: SessionMap, ev: UiEvent): SessionMap {
  switch (ev.type) {
    case 'session.updated':
      return { ...map, [ev.session.id]: ev.session };
    case 'session.removed': {
      if (!(ev.id in map)) return map;
      const next = { ...map };
      delete next[ev.id];
      return next;
    }
    case 'project.removed': {
      const next: SessionMap = {};
      for (const [id, s] of Object.entries(map)) if (s.project_id !== ev.id) next[id] = s;
      return next;
    }
    default:
      return map;
  }
}

export function sessionsToMap(list: readonly SessionInfo[]): SessionMap {
  const out: SessionMap = {};
  for (const s of list) out[s.id] = s;
  return out;
}

/**
 * Next session needing input after `afterId`, across projects (order: project order, then
 * creation time). Used by `attention.next`.
 */
export function nextNeedingInput(
  sessions: readonly SessionInfo[],
  projectOrder: readonly ProjectId[],
  afterId: SessionId | null,
): SessionInfo | null {
  const rank = (p: ProjectId): number => {
    const i = projectOrder.indexOf(p);
    return i < 0 ? projectOrder.length : i;
  };
  const candidates = sessions
    .filter((s) => s.attention === 'needs_input' || s.status === 'needs_input')
    .sort((a, b) => rank(a.project_id) - rank(b.project_id) || a.created_at.localeCompare(b.created_at));
  if (candidates.length === 0) return null;
  const at = afterId ? candidates.findIndex((s) => s.id === afterId) : -1;
  return candidates[(at + 1) % candidates.length] ?? null;
}

// ---- layout ---------------------------------------------------------------------------------

export type LayoutMap = Record<ProjectId, Layout>;

/**
 * `layout.changed` replaces the project layout (backend-initiated). `ui.open` applies the request to
 * the project's layout if it is loaded (the store then saves it).
 */
export function reduceLayouts(map: LayoutMap, ev: UiEvent): LayoutMap {
  switch (ev.type) {
    case 'layout.changed':
      return { ...map, [ev.project_id]: ev.layout };
    case 'ui.open': {
      const current = map[ev.project_id];
      if (!current) return map;
      return { ...map, [ev.project_id]: openPane(current, ev.request).layout };
    }
    case 'project.removed': {
      if (!(ev.id in map)) return map;
      const next = { ...map };
      delete next[ev.id];
      return next;
    }
    default:
      return map;
  }
}

// ---- work -----------------------------------------------------------------------------------

export type WorkMap = Record<WorkItemId, WorkItem>;

export function reduceWork(map: WorkMap, ev: UiEvent): WorkMap {
  switch (ev.type) {
    case 'work.updated':
      return { ...map, [ev.work.id]: ev.work };
    case 'project.removed': {
      const next: WorkMap = {};
      for (const [id, w] of Object.entries(map)) if (w.project_id !== ev.id) next[id] = w;
      return next;
    }
    default:
      return map;
  }
}

// ---- attention ------------------------------------------------------------------------------

export interface ProjectAttention {
  level: Attention;
  needs_input_count: number;
}

export interface AttentionState {
  byProject: Record<ProjectId, ProjectAttention>;
  totalNeedsInput: number;
}

export function attentionFromProjects(projects: readonly ProjectInfo[]): AttentionState {
  const byProject: Record<ProjectId, ProjectAttention> = {};
  let total = 0;
  for (const p of projects) {
    byProject[p.id] = { level: p.attention.level, needs_input_count: p.attention.needs_input };
    total += p.attention.needs_input;
  }
  return { byProject, totalNeedsInput: total };
}

export function reduceAttention(state: AttentionState, ev: UiEvent): AttentionState {
  switch (ev.type) {
    case 'attention.changed':
      return {
        byProject: {
          ...state.byProject,
          [ev.project_id]: { level: ev.level, needs_input_count: ev.needs_input_count },
        },
        totalNeedsInput: ev.total_needs_input,
      };
    case 'project.updated':
      return {
        ...state,
        byProject: {
          ...state.byProject,
          [ev.project.id]: {
            level: ev.project.attention.level,
            needs_input_count: ev.project.attention.needs_input,
          },
        },
      };
    case 'project.removed': {
      if (!(ev.id in state.byProject)) return state;
      const byProject = { ...state.byProject };
      const removed = byProject[ev.id];
      delete byProject[ev.id];
      return {
        byProject,
        totalNeedsInput: Math.max(0, state.totalNeedsInput - (removed?.needs_input_count ?? 0)),
      };
    }
    default:
      return state;
  }
}

// ---- accounts -------------------------------------------------------------------------------

export interface AccountState {
  status: AccountStatus;
  detail: string | null;
}

export function reduceAccounts(
  map: Record<AccountId, AccountState>,
  ev: UiEvent,
): Record<AccountId, AccountState> {
  if (ev.type !== 'account.status') return map;
  return { ...map, [ev.account_id]: { status: ev.status, detail: ev.detail } };
}
