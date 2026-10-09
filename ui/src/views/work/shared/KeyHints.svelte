<script lang="ts">
  // Keyboard hints under a list: key in mono, then what it does. Spaced, no separators.
  let { hints }: { hints: readonly (readonly [string, string])[] } = $props();
</script>

<footer class="hints" aria-hidden="true">
  {#each hints as [key, label] (key)}<span><kbd>{key}</kbd> {label}</span>{/each}
</footer>

<style>
  .hints {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-1) var(--k-space-4);
    flex: none;
    padding: var(--k-space-1) var(--k-space-3);
    background: var(--k-bezel-raised);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  kbd {
    color: var(--k-fg-muted);
  }
</style>
