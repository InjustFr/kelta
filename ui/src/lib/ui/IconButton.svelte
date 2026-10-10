<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';

  import Icon from './Icon.svelte';
  import { currentPlatform, formatChord } from './format';

  interface Props extends HTMLButtonAttributes {
    icon: string;
    /** Required accessible label (also the tooltip). */
    label: string;
    size?: 'sm' | 'md';
    active?: boolean;
    /** Shown in the tooltip, e.g. `Close pane (⌘W)`. The accessible name stays `label`. */
    chord?: string;
  }

  let { icon, label, size = 'md', active = false, chord, type = 'button', ...rest }: Props = $props();

  const tip = $derived(
    chord ? `${label} (${formatChord(chord).join(currentPlatform() === 'macos' ? '' : '+')})` : label,
  );
</script>

<button
  {...rest}
  {type}
  class="k-icon-button {size} {rest.class ?? ''}"
  class:active
  aria-label={label}
  aria-pressed={active || undefined}
  title={tip}
>
  <Icon name={icon} size={16} />
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
    color: var(--k-fg-chrome);
    cursor: pointer;
    transition: background var(--k-duration) ease-out;
  }

  .k-icon-button.sm {
    width: var(--k-control-height-sm);
    height: var(--k-control-height-sm);
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
