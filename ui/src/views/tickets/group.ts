// Tickets list grouping: by native status (ordered by category), assignee, source or none.

import type { Status, StatusCategory, TicketItem } from '$lib/gen';

export type GroupBy = 'status' | 'assignee' | 'source' | 'none';
export const GROUP_BYS: readonly GroupBy[] = ['status', 'assignee', 'source', 'none'];

export interface TicketGroup {
  id: string;
  label: string;
  /** Status grouping only: the category that orders and colours the group. */
  category: StatusCategory | null;
  items: TicketItem[];
}

/** Attention order for groups: what moves first, done last. */
const GROUP_ORDER: StatusCategory[] = ['in_progress', 'in_review', 'todo', 'unknown', 'done'];
/** Workflow order for the move menu's flow strip. */
const FLOW_ORDER: StatusCategory[] = ['todo', 'in_progress', 'in_review', 'done', 'unknown'];

/** 2px category bar colour (DESIGN: the category colours, the native name labels). */
export function categoryBar(c: StatusCategory): string {
  return c === 'in_progress'
    ? 'var(--k-info)'
    : c === 'in_review'
      ? 'var(--k-warn)'
      : c === 'done'
        ? 'var(--k-ok)'
        : 'var(--k-border-strong)';
}

/** Statuses deduped by name, in workflow order (stable within a category). */
export function flowOrder(statuses: Status[]): Status[] {
  const seen = new Map<string, Status>();
  for (const s of statuses) if (!seen.has(s.name)) seen.set(s.name, s);
  return [...seen.values()].sort((a, b) => FLOW_ORDER.indexOf(a.category) - FLOW_ORDER.indexOf(b.category));
}

/**
 * Groups in display order. `sources` maps a view id to its label and gives the source order;
 * a ticket sits under its first view.
 */
export function groupTickets(
  items: TicketItem[],
  by: GroupBy,
  sources: readonly { id: string; label: string }[] = [],
): TicketGroup[] {
  if (by === 'none') return [{ id: 'all', label: 'All', category: null, items }];
  const groups = new Map<string, TicketGroup>();
  for (const item of items) {
    const t = item.ticket;
    const [id, label, category] =
      by === 'status'
        ? [t.status.name, t.status.name, t.status.category]
        : by === 'assignee'
          ? [t.assignee?.id ?? '', t.assignee?.name ?? 'Unassigned', null]
          : [item.view_ids[0] ?? '', sources.find((s) => s.id === item.view_ids[0])?.label ?? 'Other', null];
    const g = groups.get(id) ?? { id, label, category, items: [] };
    g.items.push(item);
    groups.set(id, g);
  }
  const rank = (g: TicketGroup): number => {
    if (by === 'status') return GROUP_ORDER.indexOf(g.category ?? 'unknown');
    if (by === 'assignee') return g.id === '' ? 1 : 0; // Unassigned last
    const i = sources.findIndex((s) => s.id === g.id);
    return i < 0 ? sources.length : i;
  };
  return [...groups.values()].sort((a, b) => rank(a) - rank(b) || a.label.localeCompare(b.label));
}
