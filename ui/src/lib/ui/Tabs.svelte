<script lang="ts" generics="T extends string">
  import Icon from './Icon.svelte';

  interface Item {
    id: T;
    label: string;
    icon?: string;
    disabled?: boolean;
  }

  interface Props {
    items: readonly Item[];
    value?: T;
    label?: string;
    onchange?: (id: T) => void;
  }

  let { items, value = $bindable(), label, onchange }: Props = $props();

  /** The tab Tab lands on: the selected one, else the first enabled (nothing selected yet). */
  const focusId = $derived(
    items.find((i) => i.id === value && !i.disabled)?.id ?? items.find((i) => !i.disabled)?.id,
  );

  function select(id: T): void {
    value = id;
    onchange?.(id);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    const enabled = items.filter((i) => !i.disabled);
    const at = enabled.findIndex((i) => i.id === value);
    const next = enabled[(at + (e.key === 'ArrowRight' ? 1 : enabled.length - 1)) % enabled.length];
    if (next) {
      e.preventDefault();
      select(next.id);
    }
  }
</script>

<div class="k-tabs" role="tablist" aria-label={label} tabindex="-1" {onkeydown}>
  {#each items as item (item.id)}
    <button
      type="button"
      role="tab"
      aria-selected={item.id === value}
      tabindex={item.id === focusId ? 0 : -1}
      disabled={item.disabled}
      onclick={() => select(item.id)}
    >
      {#if item.icon}<Icon name={item.icon} size={14} />{/if}
      {item.label}
    </button>
  {/each}
</div>

<style>
  .k-tabs {
    display: flex;
    gap: var(--k-space-1);
  }

  button {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    font-weight: var(--k-weight-medium);
    white-space: nowrap;
    border: none;
    box-shadow: inset 0 -2px 0 transparent;
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
  }

  button[aria-selected='true'] {
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
    box-shadow: inset 0 -2px 0 var(--k-accent);
  }

  button:hover:not(:disabled) {
    color: var(--k-fg);
  }

  button:focus-visible {
    outline-offset: -2px;
  }
</style>
