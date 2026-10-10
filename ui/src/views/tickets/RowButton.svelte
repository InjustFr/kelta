<script lang="ts">
  import type { Snippet } from 'svelte';

  import { Icon } from '$lib/ui';

  // A control inside a list row: a span, because the row is already a <button> (buttons cannot nest).
  // The pane owns the keyboard (its keys do the same), so it stays out of the tab order.
  interface Props {
    /** Accessible name and tooltip; for a disabled control, the reason. */
    label: string;
    icon?: string;
    disabled?: boolean;
    onclick: () => void;
    children?: Snippet;
  }

  let { label, icon, disabled = false, onclick, children }: Props = $props();

  function run(e: Event): void {
    e.stopPropagation();
    if (!disabled) onclick();
  }
</script>

<span
  class="rb"
  class:icon={icon !== undefined}
  role="button"
  tabindex="-1"
  title={label}
  aria-label={label}
  aria-disabled={disabled || undefined}
  onclick={run}
  ondblclick={(e) => e.stopPropagation()}
  onkeydown={(e) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    run(e);
  }}
>
  {#if icon}<Icon name={icon} size={12} />{:else}{@render children?.()}{/if}
</span>

<style>
  .rb {
    display: inline-flex;
    align-items: center;
    border-radius: var(--k-radius-sm);
    cursor: pointer;
  }

  .icon {
    justify-content: center;
    width: 20px;
    height: 20px;
    color: var(--k-fg-muted);
  }

  .rb:hover {
    background: var(--k-bg-active);
    color: var(--k-fg);
  }

  .rb[aria-disabled='true'] {
    color: var(--k-fg-subtle);
    opacity: 0.5;
    cursor: default;
  }

  .rb[aria-disabled='true']:hover {
    background: transparent;
  }

  .rb:focus-visible {
    outline: 2px solid var(--k-focus);
    outline-offset: 1px;
  }
</style>
