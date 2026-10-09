<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  import Icon from './Icon.svelte';

  interface Props extends HTMLButtonAttributes {
    icon: string;
    /** Required accessible label (also the tooltip). */
    label: string;
    size?: 'sm' | 'md';
    active?: boolean;
  }

  let { icon, label, size = 'md', active = false, type = 'button', ...rest }: Props = $props();
</script>

<button
  {...rest}
  {type}
  class="k-icon-button {size} {rest.class ?? ''}"
  class:active
  aria-label={label}
  aria-pressed={active || undefined}
  title={label}
>
  <Icon name={icon} size={size === 'sm' ? 14 : 16} />
</button>

<style>
  .k-icon-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--k-control-height);
    height: var(--k-control-height);
    padding: 0;
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-muted);
    cursor: pointer;
  }

  .k-icon-button.sm {
    width: 22px;
    height: 22px;
  }

  .k-icon-button:hover:not(:disabled),
  .k-icon-button.active {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .k-icon-button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
