<script lang="ts">
  // Collapsed settings (a section's Advanced block, an account's or a source's rarely changed keys):
  // a chevron summary with an optional count; closed by default.
  import Icon from '$lib/ui/Icon.svelte';
  import type { Snippet } from 'svelte';
  import type { HTMLDetailsAttributes } from 'svelte/elements';

  interface Props extends HTMLDetailsAttributes {
    label: string;
    count?: number;
    children: Snippet;
  }

  let { label, count, children, ...rest }: Props = $props();
</script>

<details class="s-disclosure" {...rest}>
  <summary>
    <Icon name="chevron-right" size={14} />
    {label}
    {#if count !== undefined}<span class="k-num count">{count}</span>{/if}
  </summary>
  <div class="body">{@render children()}</div>
</details>

<style>
  summary {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    width: fit-content;
    min-height: var(--k-control-height-sm);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg-muted);
    cursor: pointer;
    list-style: none;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  summary:hover {
    color: var(--k-fg);
  }

  summary :global(svg) {
    transition: transform var(--k-duration) ease-out;
  }

  .s-disclosure[open] > summary :global(svg) {
    transform: rotate(90deg);
  }

  .count {
    font-weight: 400;
    color: var(--k-fg-subtle);
  }

  .body {
    display: flex;
    flex-direction: column;
  }
</style>
