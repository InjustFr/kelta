<script lang="ts">
  import type { Ticket, Transition } from '$lib/gen';
  import { Kbd } from '$lib/ui';

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

  let el = $state<HTMLDivElement>();
  const flowId = $props.id();
  let active = $state(-1);
  /** A move picked (digit or Enter) before the transitions arrived. */
  let pending = $state(-1);

  const moves = $derived((transitions ?? []).filter((t) => t.to.name !== ticket.status.name));
  const flow = $derived(flowOrder([ticket.status, ...(transitions ?? []).map((t) => t.to)]));

  $effect(() => {
    if (transitions && pending >= 0) choose(pending);
  });

  const pos = $derived.by(() => {
    const w = el?.offsetWidth ?? 240;
    const h = el?.offsetHeight ?? (moves.length + 2) * 26;
    return {
      left: Math.max(4, Math.min(x, window.innerWidth - w - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - h - 4)),
    };
  });

  $effect(() => {
    el?.focus();
  });

  function choose(i: number): void {
    if (!transitions && i >= 0) pending = i; // still loading: keep the key for when the moves arrive
    const t = moves[i];
    if (!t) return;
    onselect(t);
    onclose();
  }

  function onkeydown(e: KeyboardEvent): void {
    const n = moves.length;
    if (e.key === 'ArrowDown' || e.key === 'j') active = n === 0 ? -1 : (active + 1) % n;
    else if (e.key === 'ArrowUp' || e.key === 'k') active = n === 0 ? -1 : (active - 1 + n) % n;
    else if (e.key === 'Enter' || e.key === ' ') choose(active >= 0 ? active : e.key === 'Enter' ? 0 : -1);
    else if (e.key === 'Escape') onclose();
    else if (/^[1-9]$/.test(e.key)) choose(Number(e.key) - 1);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onpointerdown={onclose} oncontextmenu={(e) => (e.preventDefault(), onclose())}></div>
<div
  bind:this={el}
  class="k-move"
  role="menu"
  aria-label={`Move ${ticket.ref.key}`}
  aria-describedby={flowId}
  tabindex="-1"
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  {onkeydown}
>
  <p class="flow" id={flowId}>
    {#each flow as s (s.name)}
      <span
        class:current={s.name === ticket.status.name}
        style:--bar={categoryBar(s.category)}
        aria-current={s.name === ticket.status.name ? 'step' : undefined}>{s.name}</span
      >
    {/each}
  </p>
  {#each moves as t, i (t.id)}
    <button
      type="button"
      role="menuitem"
      class:active={i === active}
      title={`Move to ${t.to.name}`}
      onpointerenter={() => (active = i)}
      onclick={() => choose(i)}
    >
      <span class="bar" style:--bar={categoryBar(t.to.category)}></span>
      <span class="label">{t.to.name}</span>
      {#if t.name !== t.to.name}<span class="via">{t.name}</span>{/if}
      {#if i < 9}<Kbd chord={String(i + 1)} />{/if}
    </button>
  {:else}
    <p class="none">{transitions ? `No move available from ${ticket.status.name}.` : 'Loading…'}</p>
  {/each}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--k-z-menu);
  }

  .k-move {
    position: fixed;
    z-index: var(--k-z-menu);
    min-width: 220px;
    max-width: 420px;
    padding: var(--k-space-2);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
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

  button {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 100%;
    height: var(--k-row-height);
    padding: 0 var(--k-space-3);
    border: none;
    border-radius: var(--k-radius-sm);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  button.active {
    background: var(--k-bg-selected);
  }

  .bar {
    flex: none;
    width: 2px;
    height: 14px;
    background: var(--bar);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .via {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  button :global(.k-kbd) {
    margin-left: auto;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xs);
  }

  .none {
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    color: var(--k-fg-muted);
  }
</style>
