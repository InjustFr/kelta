// Claude usage from statusline snapshots (ticket #135): context, cost and the rate-limit strip.

import type { ClaudeUsage, RateWindow, SessionInfo, WorkItem } from '$lib/gen';

/** Context use above this: Claude compacts soon. */
export const CTX_AMBER = 85;

export const usd = (n: number): string => `$${n.toFixed(2)}`;

export const ctxHot = (u: ClaudeUsage): boolean => (u.context_pct ?? 0) > CTX_AMBER;

/** Saved spend of the item's ended sessions plus the unsaved spend of its sessions. */
export function itemCost(item: WorkItem, all: readonly SessionInfo[]): number {
  return all
    .filter((s) => s.work_item_id === item.id)
    .reduce((sum, s) => sum + (s.claude?.usage?.unsaved_usd ?? 0), item.cost_usd);
}

export const overBudget = (cost: number, budget: number | null | undefined): boolean =>
  budget != null && cost > budget;

/** The freshest window across sessions (one account): latest reset, then highest use; past ones are stale. */
export function latestWindow(
  all: readonly SessionInfo[],
  key: 'five_hour' | 'seven_day',
  nowSec = Date.now() / 1000,
): RateWindow | null {
  let best: RateWindow | null = null;
  for (const s of all) {
    const w = s.claude?.usage?.[key];
    if (!w || w.resets_at <= nowSec) continue;
    if (
      !best ||
      w.resets_at > best.resets_at ||
      (w.resets_at === best.resets_at && w.used_percentage > best.used_percentage)
    )
      best = w;
  }
  return best;
}

/** `▮▮▮▯` for 64% in 4 cells. */
export function meter(pct: number, cells = 4): string {
  const n = Math.min(cells, Math.max(0, Math.round((pct / 100) * cells)));
  return '▮'.repeat(n) + '▯'.repeat(cells - n);
}

/** Local `HH:MM` of a unix-seconds reset. */
export const clock = (resetsAt: number): string =>
  new Date(resetsAt * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
