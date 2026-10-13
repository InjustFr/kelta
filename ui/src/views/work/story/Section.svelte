<script lang="ts">
  // A Story panel section (#423): a header with a one-line summary; the body folds under it.
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    summary: string;
    /** Open when it is the item's current stage (or asks for attention); a click folds or unfolds it. */
    open?: boolean;
    testid: string;
    children?: Snippet;
  }

  let { title, summary, open = false, testid, children }: Props = $props();
</script>

<details class="section" class:bodyless={!children} {open} data-testid={testid}>
  <!-- A summary-only section has nothing to unfold. -->
  <summary onclick={(e) => !children && e.preventDefault()}>
    <span class="title">{title}</span>
    <span class="sum">{summary}</span>
  </summary>
  {#if children}<div class="body">{@render children()}</div>{/if}
</details>

<style>
  .section {
    border-bottom: 1px solid var(--k-border);
  }

  summary {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-3);
    cursor: pointer;
    list-style: none;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  .bodyless summary {
    cursor: default;
  }

  .title {
    flex: none;
    color: var(--k-fg-muted);
    font-weight: 600;
  }

  [open] > summary .title {
    color: var(--k-fg);
  }

  .sum {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--k-fg-muted);
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .body {
    padding: 0 var(--k-space-3) var(--k-space-3);
  }
</style>
