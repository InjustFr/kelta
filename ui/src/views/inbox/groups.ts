// Pure grouping helpers of the Inbox (grouped by project, unmatched items under "Other").

import type { ProjectId } from '$lib/gen';

export const OTHER = 'other';

export interface Group<T> {
  id: string;
  name: string;
  items: T[];
}

/**
 * Groups items by their first matching project (in `order`); items bound to no known project go
 * to the trailing "Other" group. Empty groups are dropped.
 */
export function groupByProject<T>(
  items: readonly T[],
  projectIds: (item: T) => readonly ProjectId[],
  order: readonly { id: ProjectId; name: string }[],
): Group<T>[] {
  const byId = new Map<string, Group<T>>(order.map((p) => [p.id, { id: p.id, name: p.name, items: [] }]));
  const other: Group<T> = { id: OTHER, name: 'Other', items: [] };
  for (const item of items) {
    const pid = projectIds(item).find((id) => byId.has(id));
    const group = pid ? byId.get(pid) : undefined;
    (group ?? other).items.push(item);
  }
  return [...byId.values(), other].filter((g) => g.items.length > 0);
}
