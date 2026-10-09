<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    tone?: 'neutral' | 'accent' | 'ok' | 'warn' | 'danger' | 'info';
    /** Render as a small dot (attention indicator) instead of a pill. */
    dot?: boolean;
    title?: string;
    children?: Snippet;
  }

  let { tone = 'neutral', dot = false, title, children }: Props = $props();
</script>

{#if dot}
  <span class="k-dot {tone}" {title} role={title ? 'img' : undefined} aria-label={title}></span>
{:else}
  <span class="k-badge {tone}" {title}>{@render children?.()}</span>
{/if}

<style>
  .k-badge {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    font-size: var(--k-font-size-xs);
    font-weight: 600;
    line-height: 1;
    white-space: nowrap;
    background: var(--k-bg-sunken);
    color: var(--k-fg-muted);
  }

  .k-dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--k-fg-subtle);
    flex: none;
  }

  .accent {
    background: var(--k-accent);
    color: var(--k-accent-fg);
  }

  .ok {
    background: var(--k-ok);
    color: #fff;
  }

  .warn {
    background: var(--k-warn);
    color: #fff;
  }

  .danger {
    background: var(--k-danger);
    color: #fff;
  }

  .info {
    background: var(--k-info);
    color: #fff;
  }
</style>
