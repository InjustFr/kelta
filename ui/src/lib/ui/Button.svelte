<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  import Icon from './Icon.svelte';
  import Spinner from './Spinner.svelte';

  interface Props extends HTMLButtonAttributes {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    size?: 'sm' | 'md';
    icon?: string;
    loading?: boolean;
    children?: Snippet;
  }

  let {
    variant = 'secondary',
    size = 'md',
    icon,
    loading = false,
    disabled = false,
    type = 'button',
    children,
    ...rest
  }: Props = $props();
</script>

<button
  {...rest}
  {type}
  class="k-button {variant} {size} {rest.class ?? ''}"
  disabled={disabled || loading}
  aria-busy={loading || undefined}
>
  {#if loading}
    <Spinner size={12} />
  {:else if icon}
    <Icon name={icon} size={14} />
  {/if}
  {#if children}<span class="label">{@render children()}</span>{/if}
</button>

<style>
  .k-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--k-space-2);
    height: var(--k-control-height);
    padding: 0 var(--k-space-4);
    border-radius: var(--k-radius);
    border: 1px solid var(--k-border);
    background: var(--k-bg-elev);
    color: var(--k-fg);
    white-space: nowrap;
    cursor: pointer;
  }

  .k-button.sm {
    height: 22px;
    padding: 0 var(--k-space-3);
    font-size: var(--k-font-size-sm);
  }

  .k-button:hover:not(:disabled) {
    background: var(--k-bg-hover);
  }

  .k-button:active:not(:disabled) {
    background: var(--k-bg-active);
  }

  .k-button.primary {
    background: var(--k-accent);
    border-color: var(--k-accent);
    color: var(--k-accent-fg);
  }

  .k-button.primary:hover:not(:disabled) {
    filter: brightness(1.08);
    background: var(--k-accent);
  }

  .k-button.danger {
    background: var(--k-danger);
    border-color: var(--k-danger);
    color: var(--k-danger-fg);
  }

  .k-button.ghost {
    background: transparent;
    border-color: transparent;
  }

  .k-button:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
