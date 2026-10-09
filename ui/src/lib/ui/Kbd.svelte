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
  }

  kbd {
    display: inline-block;
    min-width: 18px;
    padding: 0 4px;
    border: 1px solid var(--k-border);
    border-bottom-width: 2px;
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-elev);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    font-family: var(--k-font-ui);
    text-align: center;
    line-height: 16px;
  }
</style>
