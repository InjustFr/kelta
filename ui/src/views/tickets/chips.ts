// The Tickets filter bar's status and assignee chips (#376): one per value the list holds, with its count.
import { t } from '$lib/i18n/index.svelte';
import type { Person, TicketItem } from '$lib/gen';

import type { CountChip } from '../filters/filters';
import { categoryBar, flowOrder } from './group';

/** One chip per status (by name, as the Status grouping), in workflow order; the picked one stays at 0. */
export function statusChips(items: readonly TicketItem[], picked: string | null): CountChip[] {
  const n = new Map<string, number>();
  for (const i of items) {
    const k = i.ticket.status.name.toLowerCase();
    n.set(k, (n.get(k) ?? 0) + 1);
  }
  const statuses = items.map((i) => i.ticket.status);
  if (picked && !n.has(picked.toLowerCase())) statuses.push({ id: '', name: picked, category: 'unknown' });
  return flowOrder(statuses).map((s) => ({
    id: s.name,
    label: s.name,
    n: n.get(s.name.toLowerCase()) ?? 0,
    title: t('Only tickets in {status}', { status: s.name }),
    dot: categoryBar(s.category),
  }));
}

/**
 * One chip per assignee, by name. The id is what the Person filter takes: the team person's key, else
 * the tracker user id. `picked` is that filter's value, kept as a chip while it matches nothing.
 */
export function assigneeChips(
  items: readonly TicketItem[],
  people: readonly Person[],
  picked: { id: string; name: string } | null,
): CountChip[] {
  const chips = new Map<string, CountChip>();
  for (const { ticket } of items) {
    const a = ticket.assignee;
    if (!a) continue;
    const p = people.find((p) =>
      p.ids.some((i) => !i.code_host && i.account === ticket.ref.account && i.id === a.id),
    );
    const id = p?.key ?? a.id;
    const chip = chips.get(id) ?? {
      id,
      label: p?.name ?? a.name,
      n: 0,
      title: t('Only tickets assigned to {name}', { name: p?.name ?? a.name }),
      avatar: true,
    };
    chip.n += 1;
    chips.set(id, chip);
  }
  if (picked && !chips.has(picked.id))
    chips.set(picked.id, {
      id: picked.id,
      label: picked.name,
      n: 0,
      title: t('Only tickets assigned to {name}', { name: picked.name }),
      avatar: true,
    });
  return [...chips.values()].sort((a, b) => b.n - a.n || a.label.localeCompare(b.label));
}
