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
  /* Flat chip: a filled colour pill would compete with the lamps. Tone = a 2px left bar. */
  .k-badge {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    height: 18px;
    padding: 0 5px;
    border-radius: var(--k-radius-sm);
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    line-height: 1;
    white-space: nowrap;
    background: var(--k-bezel-raised);
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

  .k-badge.accent,
  .k-badge.ok,
  .k-badge.warn,
  .k-badge.danger,
  .k-badge.info {
    box-shadow: inset 2px 0 0 var(--tone);
    padding-left: 7px;
  }

  .accent {
    --tone: var(--k-accent);
  }

  .ok {
    --tone: var(--k-ok);
  }

  .warn {
    --tone: var(--k-warn);
  }

  .danger {
    --tone: var(--k-danger);
  }

  .info {
    --tone: var(--k-info);
  }

  .k-dot.accent,
  .k-dot.ok,
  .k-dot.warn,
  .k-dot.danger,
  .k-dot.info {
    background: var(--tone);
  }
</style>
