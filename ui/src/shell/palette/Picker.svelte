<script lang="ts" module>
  export interface PickerItem {
    id: string;
    label: string;
    detail?: string;
    icon?: string;
    kbd?: string;
    group?: string;
  }
</script>

<script lang="ts">
  import { Icon, Kbd, Sheet, Spinner } from '$lib/ui';

  interface Props {
    title: string;
    placeholder: string;
    items: readonly PickerItem[];
    query: string;
    onquery: (query: string) => void;
    onpick: (id: string) => void;
    onclose: () => void;
    empty?: string;
    busy?: boolean;
    testid?: string;
  }

  let {
    title,
    placeholder,
    items,
    query,
    onquery,
    onpick,
    onclose,
    empty = 'No results',
    busy = false,
    testid = 'picker',
  }: Props = $props();

  let selected = $state(0);
  let list = $state<HTMLElement>();

  $effect(() => {
    // New results: select the first one.
    void items;
    selected = 0;
  });

  $effect(() => {
    const el = list?.querySelector<HTMLElement>(`[data-index="${selected}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
      e.preventDefault();
      selected = items.length === 0 ? 0 : (selected + 1) % items.length;
    } else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
      e.preventDefault();
      selected = items.length === 0 ? 0 : (selected - 1 + items.length) % items.length;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const item = items[selected];
      if (item) onpick(item.id);
    }
  }
</script>

<Sheet {title} side="top" width={640} {onclose}>
  <div class="picker" data-testid={testid}>
    <div class="input">
      <Icon name="search" size={15} />
      <input
        type="text"
        {placeholder}
        value={query}
        oninput={(e) => onquery(e.currentTarget.value)}
        {onkeydown}
        spellcheck="false"
        autocomplete="off"
        aria-label={title}
        data-testid="{testid}-input"
      />
      {#if busy}<Spinner size={14} />{/if}
    </div>
    <div class="list" bind:this={list} role="listbox" aria-label="Results">
      {#each items as item, i (item.id)}
        {#if item.group && item.group !== items[i - 1]?.group}
          <div class="group" role="presentation">{item.group}</div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="row"
          class:selected={i === selected}
          role="option"
          tabindex="-1"
          aria-selected={i === selected}
          data-index={i}
          data-testid="{testid}-item"
          onpointermove={() => (selected = i)}
          onclick={() => onpick(item.id)}
        >
          <Icon name={item.icon ?? 'command'} size={14} />
          <span class="label">{item.label}</span>
          {#if item.detail}<span class="detail">{item.detail}</span>{/if}
          {#if item.kbd}<Kbd chord={item.kbd} />{/if}
        </div>
      {:else}
        <p class="empty">{empty}</p>
      {/each}
    </div>
  </div>
</Sheet>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    margin: calc(var(--k-space-4) * -1) calc(var(--k-space-5) * -1);
  }

  .input {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-5);
    border-bottom: 1px solid var(--k-border);
    color: var(--k-fg-muted);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: none;
    background: transparent;
    color: var(--k-fg);
    font-size: var(--k-font-size-lg);
  }

  input:focus-visible {
    outline: none;
  }

  .list {
    max-height: 52vh;
    overflow-y: auto;
    padding: 0 var(--k-space-3) var(--k-space-3);
  }

  .group {
    padding: var(--k-space-3) var(--k-space-3) var(--k-space-1);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: var(--k-row-height);
    padding: 0 var(--k-space-3);
    border-radius: var(--k-radius-sm);
    cursor: pointer;
  }

  .row.selected {
    background: var(--k-bg-selected);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--k-fg-subtle);
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-sm);
  }

  .label + :global(.k-kbd) {
    margin-left: auto;
  }

  .empty {
    margin: 0;
    padding: var(--k-space-5);
    color: var(--k-fg-subtle);
    text-align: center;
  }
</style>
