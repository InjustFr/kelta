<script lang="ts">
  // Keyboard hints under a list: the first five keys and `?`, one line, most important first; a narrow
  // pane drops the hints that no longer fit, whole. `keys.hints = false` hides the line.
  // With `onclose` it is the `?` dialog instead: every key of the view, then the global chords. Render
  // that one outside the pane's key handler, so keys typed in the dialog do not reach the list.
  import { msg, t } from '$lib/i18n/index.svelte';
  import { settings } from '$lib/stores';
  import { Dialog, Kbd } from '$lib/ui';

  import { chordFor } from '../../../shell/labels';

  // A third entry (the list's verb, keymap.ts) is not shown.
  type Hint = readonly [string, string, ...unknown[]];

  interface Props {
    /** `[keys, label]`, most useful first; alternative keys are separated by a space (`'j k'`), a sequence by `then` (`'f then s'`). */
    hints: readonly Hint[];
    /** Shows the full key list as a dialog, closed by this. */
    onclose?: () => void;
    /** The line's `?` is a button opening the dialog (#397): the keys are taught, not required. */
    onopen?: () => void;
  }

  let { hints, onclose, onopen }: Props = $props();

  const GLOBAL: readonly Hint[] = [
    ['palette.open', msg('Commands')],
    ['session.new', msg('New session')],
    ['pane.split_right', msg('Split')],
    ['pane.close', msg('Close pane')],
    ['attention.next', msg('Next waiting')],
  ];
  const global = $derived(
    GLOBAL.flatMap(([id, label]): Hint[] => {
      const chord = chordFor(id);
      return chord ? [[chord, label]] : [];
    }),
  );
  const line = $derived<Hint[]>(hints.slice(0, 5));
</script>

{#snippet keys(k: string)}
  {#each k.split(' ') as part (part)}{#if part === 'then'}<span class="then">{t('then')}</span>{:else}<Kbd
        chord={part}
      />{/if}{/each}
{/snippet}

{#if onclose}
  <Dialog title={t('All keys')} width={420} {onclose}>
    {#each [hints, global] as list, i (i)}
      {#if i === 1 && list.length}<h3>{t('Global')}</h3>{/if}
      <dl class="keys">
        {#each list as [k, label] (k)}
          <dt>{@render keys(k)}</dt>
          <dd>{t(label)}</dd>
        {/each}
      </dl>
    {/each}
  </Dialog>
{:else if settings.value()?.keys.hints !== false}
  <footer class="hints">
    <span class="line" aria-hidden="true"
      >{#each line as [k, label] (k)}<span class="hint">{@render keys(k)} {t(label)}</span>{/each}</span
    >
    <button type="button" class="hint all" tabindex="-1" onclick={onopen}
      ><Kbd chord="?" /> {t('All keys')}</button
    >
  </footer>
{/if}

<style>
  .hints {
    display: flex;
    align-items: center;
    gap: var(--k-space-5);
    flex: none;
    margin-top: auto; /* bottom of the pane even above an empty, error or loading state */
    height: var(--k-statusbar-height);
    overflow: hidden;
    padding: 0 var(--k-space-3);
    background: var(--k-bezel-raised);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
    white-space: nowrap;
  }

  /* One line tall: hints that wrap fall out of view whole; `All keys` after them always shows. */
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0 var(--k-space-5);
    min-width: 0;
    height: var(--k-statusbar-height);
    overflow: hidden;
  }

  .all {
    flex: none;
    padding: 0 var(--k-space-1);
    border: 0;
    border-radius: var(--k-radius-sm);
    background: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .all:hover {
    background: var(--k-bg-active);
    color: var(--k-fg);
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: var(--k-statusbar-height);
  }

  .then {
    padding: 0 2px;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  h3 {
    margin: var(--k-space-5) 0 var(--k-space-3);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg-muted);
  }

  .keys {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--k-space-3) var(--k-space-5);
    margin: 0;
    align-items: center;
  }

  .keys dt {
    display: inline-flex;
    gap: var(--k-space-1);
    justify-self: end;
  }

  .keys dd {
    margin: 0;
    color: var(--k-fg-muted);
  }
</style>
