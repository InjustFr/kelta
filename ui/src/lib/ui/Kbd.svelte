<script lang="ts">
  import { currentPlatform, formatChord } from './format';

  interface Props {
    /** Chord like `cmd+shift+k` / `ctrl+shift+k`, or a single key. */
    chord: string;
    platform?: 'macos' | 'linux';
  }

  let { chord, platform }: Props = $props();
  const os = $derived(platform ?? currentPlatform());
  // One quiet mark per chord: macOS writes it as one word (⌘⇧K), Linux joins the keys (Ctrl+Shift+K).
  const text = $derived(formatChord(chord, os).join(os === 'macos' ? '' : '+'));
</script>

<kbd class="k-kbd" aria-label={chord}>{text}</kbd>

<style>
  .k-kbd {
    display: inline-block;
    min-width: 18px;
    padding: 0 4px;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
    font-family: var(--k-font-mono);
    text-align: center;
    line-height: 16px;
  }
</style>
