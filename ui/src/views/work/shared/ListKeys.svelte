<script lang="ts">
  // A list's keys (#437): `?` opens the floating panel of the prefix (KeyPanel) with the list's keys
  // in the groups of the verb table (keymap.ts); its `? All keys` opens the dialog instead.
  // With `onclose` it is that `?` dialog: every key of the view, then the global chords. Render the
  // dialog outside the pane's key handler, so keys typed in it do not reach the list; the panel inside
  // the pane, which places it.
  import { msg, t } from '$lib/i18n/index.svelte';
  import { Dialog, Kbd } from '$lib/ui';

  import KeyPanel from '../../../shell/KeyPanel.svelte';
  import { chordFor } from '../../../shell/labels';
  import { panelGroups, type ListKey } from './keymap';

  type Hint = readonly [string, string, ...unknown[]];

  interface Props {
    /** `[keys, label]`, most useful first; alternative keys are separated by a space (`'j k'`), a sequence by `then` (`'f then s'`). */
    hints: readonly ListKey[];
    /** Shows the full key list as a dialog, closed by this. */
    onclose?: () => void;
    /** The panel's `? All keys` button: opens the dialog. */
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
  const panel = $derived(
    panelGroups(hints).map(({ title, keys }) => ({
      id: title,
      title: t(title),
      lines: keys.map(([k, label]) => ({ key: k, chord: true, label: t(label) })),
    })),
  );
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
{:else}
  <KeyPanel label={t('Keys')} groups={panel} data-testid="list-keys">
    {#snippet foot()}<span>{t('Esc closes')}</span><button
        type="button"
        class="all"
        tabindex="-1"
        onclick={onopen}><Kbd chord="?" /> {t('All keys')}</button
      >{/snippet}
  </KeyPanel>
{/if}

<style>
  .all {
    display: inline-flex;
    align-items: center;
    gap: 2px;
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
