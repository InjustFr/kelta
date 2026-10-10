<script lang="ts">
  import { onMount, untrack } from 'svelte';

  import type { ProjectId, Ticket, Transition } from '$lib/gen';
  import { tickets as store } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';

  import MoveDialogs from './MoveDialogs.svelte';
  import MoveMenu from './MoveMenu.svelte';
  import { MoveController } from './move.svelte';

  // The one status picker (TICKETS.md T2): list `m`, status chips, detail, work bar, palette.
  // Fed by `tracker_transitions` per ticket; several tickets move by target status name (ids differ
  // between workflows). Stays mounted until the move and its dialogs (pick, fields) are over.
  interface Props {
    tickets: readonly Ticket[];
    /** Part of the picker's contract; transitions are per ticket, so the move does not need it. */
    projectId?: ProjectId | null;
    /** Opens under this element or at this point; none = top-anchored like the palette. */
    anchor?: HTMLElement | { x: number; y: number } | null;
    onclose: () => void;
  }

  let { tickets, anchor = null, onclose }: Props = $props();

  const move = new MoveController();
  let open = $state(true);
  let running = $state(false);

  const pos = untrack(() => {
    if (anchor instanceof HTMLElement) {
      const r = anchor.getBoundingClientRect();
      return { x: r.left, y: r.bottom + 2 };
    }
    return anchor ?? { x: window.innerWidth / 2 - 140, y: window.innerHeight * 0.14 };
  });

  const slots = $derived(tickets.map((t) => store.transitions[ticketKey(t.ref)]));
  const error = $derived(slots.find((s) => s?.error && !s.data)?.error?.message ?? null);
  const lists = $derived(slots.map((s) => s?.data ?? null));
  const ready = $derived(lists.every((l) => l !== null));

  /** Status names match across trackers whatever their case ("In progress", "In Progress"). */
  const same = (a: string, b: string): boolean => a.toLowerCase() === b.toLowerCase();

  /** One transition per target name that every ticket can reach (or already sits in). */
  const shared = $derived.by((): Transition[] | null => {
    if (!ready) return null;
    const flat = lists.flatMap((l) => l ?? []);
    if (tickets.length < 2) return flat;
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local dedupe, not state
    const reps = new Map<string, Transition>();
    for (const t of flat) if (!reps.has(t.to.name.toLowerCase())) reps.set(t.to.name.toLowerCase(), t);
    return [...reps.values()].filter((rep) =>
      tickets.every(
        (t, i) => same(t.status.name, rep.to.name) || lists[i]?.some((u) => same(u.to.name, rep.to.name)),
      ),
    );
  });

  onMount(() => {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local dedupe, not state
    const seen = new Set<string>();
    for (const t of tickets) {
      const k = ticketKey(t.ref);
      if (!seen.has(k)) void store.loadTransitions(t.ref);
      seen.add(k);
    }
  });

  async function pick(rep: Transition): Promise<void> {
    const moves = tickets.flatMap((ticket, i) => {
      if (same(ticket.status.name, rep.to.name) && tickets.length > 1) return [];
      const transition = tickets.length > 1 ? lists[i]?.find((u) => same(u.to.name, rep.to.name)) : rep;
      return transition ? [{ ticket, transition }] : [];
    });
    running = true;
    try {
      await move.moveAll(moves);
    } finally {
      running = false;
    }
  }

  $effect(() => {
    if ((!open || tickets.length === 0) && !running && !move.dialog && !move.busy) onclose();
  });
</script>

{#if open && tickets[0]}
  <MoveMenu
    ticket={tickets[0]}
    selection={tickets}
    transitions={shared}
    {error}
    x={pos.x}
    y={pos.y}
    onselect={(t) => void pick(t)}
    onclose={() => (open = false)}
  />
{/if}
<MoveDialogs {move} />
