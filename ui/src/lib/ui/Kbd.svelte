<script lang="ts">
  import { formatChord } from './format';

  interface Props {
    /** Chord like `cmd+shift+k` / `ctrl+shift+k`, or a single key. */
    chord: string;
    platform?: 'macos' | 'linux';
  }

  let { chord, platform }: Props = $props();
  const parts = $derived(formatChord(chord, platform));
</script>

<span class="k-kbd" aria-label={chord}>
  {#each parts as part, i (i)}<kbd>{part}</kbd>{/each}
</span>

<style>
  .k-kbd {
    display: inline-flex;
    gap: 2px;
    color: var(--k-fg-subtle);
  }

  kbd {
    display: inline-block;
    min-width: 12px;
    padding: 0 2px;
    border-bottom: 1px solid var(--k-border-strong);
    color: inherit;
    font-size: var(--k-font-size-xs);
    font-family: var(--k-font-mono);
    text-align: center;
    line-height: 15px;
  }
</style>
