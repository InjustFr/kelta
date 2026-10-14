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
  import KeyPanel from './KeyPanel.svelte';

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

  const panel = groups.map(({ group, hints }) => ({
    id: group,
    title: title(group),
    lines: lines(group, hints).map((h) => ({ key: h.key, chord: h.chord, label: label(h) })),
  }));
</script>

<KeyPanel label={t('Prefix keys')} groups={panel} data-testid="which-key">
  {#snippet head()}<Kbd chord={prefix} /> <b>{t('then one key')}</b>{/snippet}
  {#snippet foot()}<span>{t('Esc cancels')}</span><span>{t('? lists every key')}</span><span
      >{t('Rebind in Settings › Keys')}</span
    >{/snippet}
</KeyPanel>
