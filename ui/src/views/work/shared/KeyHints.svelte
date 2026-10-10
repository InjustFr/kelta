<script lang="ts">
  // Keyboard hints under a list: key caps, then what the key does. Most important first: a narrow
  // pane keeps one line and drops the hints that no longer fit, whole.
  import { Kbd } from '$lib/ui';

  /** `[keys, label]`; alternative keys are separated by a space (`'j k'`), a sequence by `then` (`'f then s'`). */
  let { hints }: { hints: readonly (readonly [string, string])[] } = $props();
</script>

<footer class="hints" aria-hidden="true">
  {#each hints as [keys, label] (keys)}<span class="hint"
      >{#each keys.split(' ') as k (k)}{#if k === 'then'}<span class="then">then</span>{:else}<Kbd
            chord={k}
          />{/if}{/each}
      {label}</span
    >{/each}
</footer>

<style>
  .hints {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0 var(--k-space-5);
    flex: none;
    /* One line tall: hints that wrap fall out of view instead of adding a second line. */
    height: var(--k-statusbar-height);
    overflow: hidden;
    padding: 0 var(--k-space-3);
    background: var(--k-bezel-raised);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
    white-space: nowrap;
  }

  .then {
    padding: 0 2px;
    color: var(--k-fg-subtle);
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: var(--k-statusbar-height);
  }
</style>
