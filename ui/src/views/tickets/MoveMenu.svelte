<script lang="ts">
  import type { Ticket, Transition } from '$lib/gen';
  import { Menu, type MenuItem } from '$lib/ui';

  import { categoryBar, flowOrder } from './group';

  // Move menu: the ticket's own workflow as a strip (current status lit), then the legal
  // transitions numbered 1-9 (a digit moves at once).
  interface Props {
    ticket: Ticket;
    /** null while loading. */
    transitions: readonly Transition[] | null;
    x: number;
    y: number;
    onselect: (t: Transition) => void;
    onclose: () => void;
  }

  let { ticket, transitions, x, y, onselect, onclose }: Props = $props();

  /** A move picked (digit or Enter) before the transitions arrived. */
  let pending = $state(-1);

  const moves = $derived((transitions ?? []).filter((t) => t.to.name !== ticket.status.name));
  const flow = $derived(flowOrder([ticket.status, ...(transitions ?? []).map((t) => t.to)]));
  const items = $derived<MenuItem[]>(
    moves.map((t, i) => ({
      id: t.id,
      label: t.to.name,
      detail: t.name !== t.to.name ? t.name : undefined,
      bar: categoryBar(t.to.category),
      title: `Move to ${t.to.name}`,
      key: i < 9 ? String(i + 1) : undefined,
      kbd: i < 9 ? String(i + 1) : undefined,
    })),
  );

  $effect(() => {
    const t = transitions && moves[pending];
    if (!t) return;
    onselect(t);
    onclose();
  });

  /** Still loading: keep a digit or Enter for when the moves arrive. */
  function holdKey(e: KeyboardEvent): void {
    if (transitions || !/^([1-9]|Enter)$/.test(e.key)) return;
    pending = e.key === 'Enter' ? 0 : Number(e.key) - 1;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div class="hold" onkeydowncapture={holdKey}>
  <Menu
    {items}
    {x}
    {y}
    label={`Move ${ticket.ref.key}`}
    onselect={(id) => {
      const t = moves.find((m) => m.id === id);
      if (t) onselect(t);
    }}
    {onclose}
  >
    {#snippet header()}
      <p class="flow">
        {#each flow as s (s.name)}
          <span
            class:current={s.name === ticket.status.name}
            style:--bar={categoryBar(s.category)}
            aria-current={s.name === ticket.status.name ? 'step' : undefined}>{s.name}</span
          >
        {/each}
      </p>
      {#if moves.length === 0}
        <p class="none">{transitions ? `No move available from ${ticket.status.name}.` : 'Loading…'}</p>
      {/if}
    {/snippet}
  </Menu>
</div>

<style>
  .hold {
    display: contents;
  }

  /* The flow strip: the one line that shows where the ticket is in its own workflow. */
  .flow {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2) var(--k-space-4);
    margin: 0 0 var(--k-space-2);
    padding: var(--k-space-2) var(--k-space-3) var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-subtle);
    white-space: nowrap;
  }

  .flow .current {
    padding-left: 6px;
    box-shadow: inset 2px 0 0 var(--bar);
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
  }

  .none {
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    color: var(--k-fg-muted);
  }
</style>
