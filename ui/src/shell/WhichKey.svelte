<script lang="ts">
  // which-key panel (#416): shown after `keys.prefix_hint_ms` without a second key; the prefix stays
  // armed until a key or Esc. Built when it mounts, so a rebind (which disarms) shows on the next prefix.
  import { t } from '$lib/i18n/index.svelte';
  import { keyManager, type HintGroup, type PrefixHint } from '$lib/keys';
  import { projects, tools } from '$lib/stores';
  import { Kbd } from '$lib/ui';

  import { focusedWorkItem } from '../views/work/actions';
  import { phaseNow } from '../views/work/live';
  import { workKey } from '../views/work/phase';

  const item = focusedWorkItem();
  const { prefix, groups } = keyManager.prefixHints(item !== null);
  // The tools with a key of their own, before Settings; the group stays within 6 lines.
  const toolHints: PrefixHint[] = tools
    .list(projects.activeId ?? '')
    .filter((x) => x.keybinding)
    .slice(0, 5)
    .map((x) => ({ key: x.keybinding!, action: 'tools.open', label: x.label, chord: true }));

  function lines(group: HintGroup, hints: PrefixHint[]): PrefixHint[] {
    return group === 'tools' ? [...toolHints, ...hints] : hints;
  }

  /** The next step names itself; the other labels are the table's. */
  function label(h: PrefixHint): string {
    return h.action === 'work.next' && item ? phaseNow(item).primaryLabel || t('Go to work tab') : t(h.label);
  }

  function title(group: HintGroup): string {
    switch (group) {
      case 'goto':
        return t('Go to');
      case 'work':
        return t('This work: {key}', { key: item ? workKey(item) : '' });
      case 'panes':
        return t('Panes');
      case 'tools':
        return t('Tools here');
    }
  }
</script>

<div class="wk" role="status" aria-label={t('Prefix keys')} data-testid="which-key">
  <div class="head"><Kbd chord={prefix} /> <b>{t('then one key')}</b></div>
  <div class="cols">
    {#each groups as { group, hints } (group)}
      <section data-group={group}>
        <h5>{title(group)}</h5>
        <dl>
          {#each lines(group, hints) as h (h.action + h.key)}
            <dt class="key">
              {#if h.chord}<Kbd chord={h.key} />{:else}{h.key}{/if}
            </dt>
            <dd>{label(h)}</dd>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
  <div class="foot">
    <span>{t('Esc cancels')}</span><span>{t('? lists every key')}</span><span
      >{t('Rebind in Settings › Keys')}</span
    >
  </div>
</div>

<style>
  .wk {
    position: absolute;
    left: 50%;
    bottom: calc(var(--k-statusbar-height) + var(--k-space-3));
    transform: translateX(-50%);
    z-index: var(--k-z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    max-width: calc(100% - 2 * var(--k-space-5));
    max-height: 70%;
    overflow: auto;
    padding: var(--k-space-4) var(--k-space-5);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
    pointer-events: none;
  }

  .head {
    display: flex;
    gap: var(--k-space-3);
    align-items: center;
  }

  .cols {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-4) var(--k-space-6);
  }

  h5 {
    margin: 0 0 var(--k-space-2);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    font-weight: var(--k-weight-strong);
  }

  dl {
    display: grid;
    grid-template-columns: auto auto;
    gap: var(--k-space-1) var(--k-space-3);
    margin: 0;
  }

  dd {
    margin: 0;
    white-space: nowrap;
  }

  .key {
    min-width: 18px;
    color: var(--k-accent);
    font-family: var(--k-font-mono);
    text-align: center;
  }

  .foot {
    display: flex;
    gap: var(--k-space-5);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
  }
</style>
