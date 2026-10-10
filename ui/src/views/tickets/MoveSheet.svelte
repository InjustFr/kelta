<script lang="ts">
  import { onMount } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type { Ticket, Transition } from '$lib/gen';
  import { tickets, toasts } from '$lib/stores';

  import MoveDialogs from './MoveDialogs.svelte';
  import MoveMenu from './MoveMenu.svelte';
  import { MoveController } from './move.svelte';

  // Sheet `tickets.move` (the palette's "Move KEY to…"): the panes' move menu and dialogs, without a pane.
  let { ticket, onclose }: SheetProps & { ticket: Ticket } = $props();

  const move = new MoveController();
  let transitions = $state<Transition[] | null>(null);
  let menuOpen = $state(true);

  onMount(async () => {
    const slot = await tickets.loadTransitions(ticket.ref);
    if (slot.data) transitions = slot.data;
    else {
      menuOpen = false;
      toasts.error(slot.error ?? 'No transitions', `Moving ${ticket.ref.key}`);
    }
  });

  // Stay mounted while a move runs or asks (pick / fields dialog), close once it is all over.
  $effect(() => {
    if (!menuOpen && !move.dialog && !move.busy) onclose();
  });
</script>

{#if menuOpen}
  <MoveMenu
    {ticket}
    {transitions}
    x={window.innerWidth / 2 - 120}
    y={window.innerHeight / 4}
    onselect={(t) => void move.moveViaTransition(ticket, t)}
    onclose={() => (menuOpen = false)}
  />
{/if}
<MoveDialogs {move} />
