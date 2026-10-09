// Phase rows 6, 7, 15 and 16 of FLOW §2.2 (Ship and clean up), as a pure function.
// shortcut: the full `phaseOf` (views/work/phase.ts, now-view lane) is not on main yet; it should
// return `shipPhase(...)` for these rows once it lands, so the rules live in one place.

import type { GitStatus, Review, SessionInfo, WorkItem } from '$lib/gen';

export interface ShipPhase {
  id: 'merged' | 'pr_closed' | 'ready_to_ship' | 'approved';
  label: string;
  detail: string;
  lamp: 'none';
  /** Action id run by the row's Enter / the primary button (`open_pr` = Open PR on the host). */
  primary: 'work.finish' | 'work.ship' | 'open_pr';
  section: 'ship';
}

/** `#13` from a PR / MR URL (`…/pull/13`, `…/merge_requests/13`), or `''`. */
export function prNumber(url: string | null | undefined): string {
  const n = url?.replace(/\/+$/, '').split('/').pop() ?? '';
  return /^\d+$/.test(n) ? `#${n}` : '';
}

/** A Claude session of the item is mid-turn or waiting on a permission prompt (Ship refused). */
export function claudeBusy(item: WorkItem, session: (id: string) => SessionInfo | null): boolean {
  return item.session_ids.some((id) => {
    const s = session(id);
    return s?.kind.type === 'claude' && (s.status === 'working' || s.status === 'needs_input');
  });
}

export function shipPhase(item: WorkItem, pr: Review | null, git: GitStatus | null): ShipPhase | null {
  const n = prNumber(item.pr_url);
  const row = (
    id: ShipPhase['id'],
    label: string,
    detail: string,
    primary: ShipPhase['primary'],
  ): ShipPhase => ({
    id,
    label,
    detail,
    lamp: 'none',
    primary,
    section: 'ship',
  });
  if (item.state.kind === 'merged') {
    const moved = item.ticket ? ', ticket moved to Done' : '';
    return row('merged', 'Merged', item.state.detail ?? `${n}${moved}`, 'work.finish');
  }
  if (item.state.kind === 'pr_closed')
    return row('pr_closed', 'PR closed', `${n} closed without merge`, 'work.finish');
  if (!item.pr_url && git && git.ahead > 0) {
    return row(
      'ready_to_ship',
      'Ready to ship',
      `${git.ahead} commit${git.ahead === 1 ? '' : 's'}`,
      'work.ship',
    );
  }
  if (item.pr_url && pr?.decision === 'approved' && (pr.ci === 'success' || pr.ci === 'none')) {
    return row('approved', 'Approved', n, 'open_pr');
  }
  return null;
}
