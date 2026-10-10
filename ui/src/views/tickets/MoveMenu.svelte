<script lang="ts">
  import type { Ticket, Transition } from '$lib/gen';
  import { openExternal } from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import { Button, Menu, type MenuItem } from '$lib/ui';

  import { rank } from '../../shell/palette/fuzzy';
  import { categoryBar, flowOrder } from './group';

  // The status picker's menu (StatusPicker wraps it with loading, moves and dialogs): the ticket's
  // own workflow as a strip (current status lit), a filter, then the legal transitions numbered
  // 1-9 (a digit moves at once).
  interface Props {
    ticket: Ticket;
    /** Several tickets: their statuses are lit and `transitions` are the moves they share. */
    selection?: readonly Ticket[];
    /** null while loading. */
    transitions: readonly Transition[] | null;
    /** The tracker's message when the transitions could not be loaded. */
    error?: string | null;
    x: number;
    y: number;
    onselect: (t: Transition) => void;
    onclose: () => void;
  }

  let { ticket, selection, transitions, error = null, x, y, onselect, onclose }: Props = $props();

  /** A move picked (digit or Enter) before the transitions arrived. */
  let pending = $state(-1);
  let query = $state('');
  let input = $state<HTMLInputElement>();

  const all = $derived(selection && selection.length > 0 ? selection : [ticket]);
  const lit = $derived(all.map((t) => t.status.name));
  const label = $derived(all.length > 1 ? `Move ${all.length} tickets` : `Move ${ticket.ref.key}`);
  const moves = $derived(
    (transitions ?? []).filter((t) => !lit.every((n) => n.toLowerCase() === t.to.name.toLowerCase())),
  );
  const shown = $derived(rank(moves, query, (t) => `${t.to.name} ${t.name}`));
  const flow = $derived(flowOrder([...all.map((t) => t.status), ...(transitions ?? []).map((t) => t.to)]));
  const items = $derived<MenuItem[]>(
    shown.map((t, i) => ({
      id: t.id,
      label: t.to.name,
      detail: [t.name !== t.to.name ? t.name : '', t.needs_fields ? 'asks for fields' : '']
        .filter(Boolean)
        .join(', '),
      bar: categoryBar(t.to.category),
      title: `Move to ${t.to.name}`,
      key: i < 9 ? String(i + 1) : undefined,
      kbd: i < 9 ? String(i + 1) : undefined,
    })),
  );

  $effect(() => {
    const t = transitions && shown[pending];
    if (!t) return;
    onselect(t);
    onclose();
  });

  // The Menu focuses itself on mount; the filter takes the keyboard right after.
  $effect(() => {
    if (input) queueMicrotask(() => input?.focus());
  });

  /** Still loading: keep a digit or Enter for when the moves arrive. */
  function holdKey(e: KeyboardEvent): void {
    if (transitions || !/^([1-9]|Enter)$/.test(e.key)) return;
    pending = e.key === 'Enter' ? 0 : Number(e.key) - 1;
    e.preventDefault();
    e.stopPropagation();
  }

  /** Text editing stays in the filter; digits, arrows, Enter and Esc go on to the menu. */
  function filterKey(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const edit = e.key.length === 1 && !/^[1-9]$/.test(e.key);
    if (edit || /^(Backspace|Delete|ArrowLeft|ArrowRight|Home|End)$/.test(e.key)) e.stopPropagation();
  }

  function browse(): void {
    openExternal({ url: ticket.url }).catch((err) => toasts.error(err, 'Open in browser'));
  }
</script>

<div class="hold" onkeydowncapture={holdKey}>
  <Menu
    {items}
    {x}
    {y}
    {label}
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
            class:current={lit.includes(s.name)}
            style:--bar={categoryBar(s.category)}
            aria-current={lit.includes(s.name) ? 'step' : undefined}>{s.name}</span
          >
        {/each}
      </p>
      {#if error}
        <div class="none">
          <p>{error}</p>
          <Button size="sm" variant="ghost" icon="external-link" onclick={browse}>Open in browser</Button>
        </div>
      {:else}
        <input
          bind:this={input}
          bind:value={query}
          class="filter"
          placeholder="Filter statuses"
          aria-label="Filter statuses"
          spellcheck="false"
          autocomplete="off"
          onkeydown={filterKey}
        />
        {#if shown.length === 0}
          <div class="none">
            {#if !transitions}
              <p>Loading…</p>
            {:else if moves.length > 0}
              <p>No status matches "{query}".</p>
            {:else}
              <p>
                {all.length > 1
                  ? 'These tickets have no status in common to move to.'
                  : `No move available from ${ticket.status.name}.`}
              </p>
              <Button size="sm" variant="ghost" icon="external-link" onclick={browse}>Open in browser</Button>
            {/if}
          </div>
        {/if}
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
    margin: 0;
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

  /* Typed query in mono (DESIGN §6.6), flush under the strip like the palette's input. */
  .filter {
    display: block;
    width: 100%;
    height: var(--k-row-height);
    margin: 0 0 var(--k-space-2);
    padding: 0 var(--k-space-3);
    border: 0;
    border-bottom: 1px solid var(--k-border);
    background: transparent;
    color: var(--k-fg);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
  }

  .filter::placeholder {
    color: var(--k-fg-subtle);
    font-family: var(--k-font-ui, inherit);
  }

  .filter:focus-visible {
    outline: none;
  }

  .none {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-2);
    padding: var(--k-space-2) var(--k-space-3);
    color: var(--k-fg-muted);
  }

  .none p {
    margin: 0;
    max-width: 32ch;
    white-space: normal;
  }
</style>
