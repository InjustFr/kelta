// Helpers of the Ship and Finish dialogs.

import type { SessionInfo, WorkItem } from '$lib/gen';

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
