<script lang="ts">
  import { currentPlatform, formatChord } from './format';

  interface Props {
    /** Chord like `cmd+shift+k` / `ctrl+shift+k`, or a single key. */
    chord: string;
    platform?: 'macos' | 'linux';
  }

  let { chord, platform }: Props = $props();
  const os = $derived(platform ?? currentPlatform());
  // macOS writes a chord as one word (⌘⇧K); Linux keeps one cap per key (Ctrl Shift K).
  const parts = $derived(os === 'macos' ? [formatChord(chord, os).join('')] : formatChord(chord, os));
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
    min-width: 18px;
    min-height: 18px;
    padding: 0 4px;
    border: 1px solid var(--k-border);
    border-bottom-width: 2px;
    border-radius: var(--k-radius-sm);
    color: inherit;
    font-size: var(--k-font-size-xs);
    font-family: var(--k-font-mono);
    text-align: center;
    line-height: 16px;
  }
</style>
