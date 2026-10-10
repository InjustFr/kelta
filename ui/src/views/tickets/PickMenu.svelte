<script lang="ts">
  import { untrack } from 'svelte';

  import { Menu, type MenuItem } from '$lib/ui';

  // A menu with a filter that asks `load` for its items as the filter changes (TICKETS.md T8: the
  // assignee and priority pickers). The last answer wins; a failure shows the tracker's message.
  interface Props {
    label: string;
    placeholder: string;
    x: number;
    y: number;
    load: (query: string) => Promise<MenuItem[]>;
    onselect: (id: string) => void;
    onclose: () => void;
  }

  let { label, placeholder, x, y, load, onselect, onclose }: Props = $props();

  let query = $state('');
  let items = $state<MenuItem[] | null>(null);
  let error = $state<string | null>(null);
  let input = $state<HTMLInputElement>();
  let seq = 0;

  $effect(() => {
    const q = query.trim();
    const mine = ++seq;
    // one-shot: debounce typing (not the first load), last request wins
    const timer = setTimeout(
      () => {
        load(q).then(
          (found) => {
            if (mine !== seq) return;
            items = found;
            error = null;
          },
          (err: unknown) => {
            if (mine !== seq) return;
            items = [];
            error = err instanceof Error ? err.message : String(err);
          },
        );
      },
      // untracked: each answer writes `items`, and tracking it would reload forever
      untrack(() => items) === null ? 0 : 200,
    );
    return () => clearTimeout(timer);
  });

  // The Menu focuses itself on mount; the filter takes the keyboard right after.
  $effect(() => {
    if (input) queueMicrotask(() => input?.focus());
  });

  /** Text editing stays in the filter; arrows, Enter and Esc go on to the menu. */
  function filterKey(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key.length === 1 || /^(Backspace|Delete|ArrowLeft|ArrowRight|Home|End)$/.test(e.key))
      e.stopPropagation();
  }
</script>

<Menu items={items ?? []} {x} {y} {label} filtered {onselect} {onclose}>
  {#snippet header()}
    <input
      bind:this={input}
      bind:value={query}
      class="filter"
      {placeholder}
      aria-label={placeholder}
      spellcheck="false"
      autocomplete="off"
      onkeydown={filterKey}
    />
    {#if error}
      <p class="none">{error}</p>
    {:else if !items}
      <p class="none">Loading…</p>
    {:else if items.length === 0}
      <p class="none">No match{query.trim() ? ` for "${query.trim()}"` : ''}.</p>
    {/if}
  {/snippet}
</Menu>

<style>
  /* Same filter as the status picker (MoveMenu). */
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
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-sm);
  }
</style>
