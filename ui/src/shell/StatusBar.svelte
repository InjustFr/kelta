<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SessionInfo } from '$lib/gen';
  import { projects, sessions, settings, work } from '$lib/stores';

  import { claudeSlots } from '../views/work/batch.svelte';
  import { sessionLabel } from '../views/work/live';
  import { clock, itemCost, latestWindow, meter as bar, usd } from './usage';

  // Exceptions only: no key hints here (the views' `?` dialogs and the palette list the global chords).
  let { prefixArmed }: { prefixArmed: boolean } = $props();

  const restart = $derived(settings.pendingRestart.length > 0);
  const slots = $derived(claudeSlots());

  // Rate-limit strip (subscription accounts only: API-key sessions report no rate_limits).
  const five = $derived(latestWindow(sessions.all, 'five_hour'));
  const seven = $derived(latestWindow(sessions.all, 'seven_day'));
  const spendTitle = $derived.by(() => {
    const cost = (s: SessionInfo): number => s.claude?.usage?.cost_usd ?? 0;
    const top = sessions.all
      .filter((s) => cost(s) > 0)
      .sort((a, b) => cost(b) - cost(a))
      .slice(0, 5)
      .map((s) => `${sessionLabel(s)}  ${usd(cost(s))}`);
    const perProject: Record<string, number> = {};
    for (const w of work.all)
      perProject[w.project_id] = (perProject[w.project_id] ?? 0) + itemCost(w, sessions.all);
    const projectLines = Object.entries(perProject)
      .filter(([, v]) => v > 0)
      .map(([id, v]) => `${projects.byId(id)?.name ?? id}  ${usd(v)}`);
    return [
      t('Top sessions by spend'),
      ...(top.length ? top : [t('none yet')]),
      ...(projectLines.length ? ['', t('Work items per project'), ...projectLines] : []),
    ].join('\n');
  });
</script>

<footer class="statusbar k-num" data-testid="statusbar">
  {#if prefixArmed}
    <span class="seg prefix" data-testid="status-prefix" role="status">{t('Prefix: press a key')}</span>
  {/if}
  {#if restart}
    <span class="seg warn" title={t('Some settings need a restart')}>{t('Restart needed')}</span>
  {/if}
  {#if slots.live > 0 || slots.queued > 0}
    <span
      class="seg"
      data-testid="status-slots"
      title={t('Live Claude processes / claude.max_live, and work items waiting for a slot')}
    >
      Claude {slots.live}{slots.max ? `/${slots.max}` : ''}{slots.queued
        ? ` · ${t('{n} queued', { n: slots.queued })}`
        : ''}
    </span>
  {/if}
  {#if five || seven}
    <span class="seg" data-testid="status-rate" title={spendTitle}>
      {#if five}<span
          >{t('5h')} <span aria-hidden="true">{bar(five.used_percentage)}</span>
          {Math.round(five.used_percentage)}% → {clock(five.resets_at)}</span
        >{/if}
      {#if five && seven}<span aria-hidden="true">·</span>{/if}
      {#if seven}<span>{t('7d')} {Math.round(seven.used_percentage)}%</span>{/if}
    </span>
  {/if}
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--k-space-4);
    height: var(--k-statusbar-height);
    flex: none;
    padding: 0 var(--k-space-3);
    background: var(--k-bezel-raised);
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-xs);
    white-space: nowrap;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    flex: none;
  }

  .warn {
    color: var(--k-warn);
  }

  .prefix {
    color: var(--k-accent);
    font-weight: var(--k-weight-strong);
  }
</style>
