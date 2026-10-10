<script lang="ts">
  import type { TerminalView } from '$lib/terminal/view';
  import { IconButton } from '$lib/ui';

  interface Props {
    view: TerminalView;
    onclose: () => void;
  }

  let { view, onclose }: Props = $props();

  let query = $state('');
  let caseSensitive = $state(false);
  let found = $state<boolean | null>(null);
  let input = $state<HTMLInputElement>();

  $effect(() => {
    input?.focus();
    input?.select();
  });

  async function find(backwards = false): Promise<void> {
    found = query === '' ? null : await view.find(query, { backwards, caseSensitive });
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      void find(e.shiftKey);
    }
  }
</script>

<div class="search" role="search" data-testid="terminal-search">
  <input
    bind:this={input}
    bind:value={query}
    type="text"
    placeholder="Search"
    aria-label="Search terminal"
    spellcheck="false"
    class:miss={found === false}
    oninput={() => void find()}
    {onkeydown}
  />
  <IconButton icon="chevron-up" label="Previous match" size="sm" onclick={() => void find(true)} />
  <IconButton icon="chevron-down" label="Next match" size="sm" onclick={() => void find()} />
  <IconButton
    icon="filter"
    label="Match case"
    size="sm"
    active={caseSensitive}
    onclick={() => {
      caseSensitive = !caseSensitive;
      void find();
    }}
  />
  <IconButton icon="x" label="Close search" size="sm" onclick={onclose} />
</div>

<style>
  .search {
    position: absolute;
    top: var(--k-space-3);
    right: var(--k-space-5);
    z-index: 4;
    display: flex;
    align-items: center;
    gap: var(--k-space-1);
    padding: var(--k-space-2);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
  }

  input {
    width: 180px;
    height: 24px;
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg);
    color: var(--k-fg);
  }

  input.miss {
    border-color: var(--k-danger);
  }
</style>
