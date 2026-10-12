<script lang="ts">
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { dispatch } from '$lib/actions';
  import type { SessionInfo } from '$lib/gen';
  import { lampOf, projects, sessions, settings, work, type LampLevel } from '$lib/stores';
  import { Lamp } from '$lib/ui';

  import { claudeSlots } from '../views/work/batch.svelte';
  import { sessionLabel } from '../views/work/live';
  import { focusedSessionId, revealSession } from './nav';
  import { clock, itemCost, latestWindow, meter as bar, usd } from './usage';

  // Exceptions only: no key hints here (the views' `?` dialogs and the palette list the global chords).
  let { prefixArmed }: { prefixArmed: boolean } = $props();

  // Lamp meter: one button per non-zero state; clicking walks the sessions in that state.
  const METER: { lamp: LampLevel; label: string }[] = [
    { lamp: 'needs_input', label: msg('need you') },
    { lamp: 'error', label: msg('in error') },
    { lamp: 'working', label: msg('working') },
    { lamp: 'done', label: msg('done') },
  ];
  const meter = $derived(
    METER.map((m) => ({
      ...m,
      list: sessions.all.filter((s) => lampOf(s.attention, s.status === 'working') === m.lamp),
    })).filter((m) => m.list.length > 0),
  );

  function words(lamp: LampLevel, n: number): string {
    if (lamp === 'needs_input') return tn('{n} needs you', '{n} need you', n);
    if (lamp === 'done') return tn('{n} ready to review', '{n} ready to review', n);
    if (lamp === 'error') return tn('{n} in error', '{n} in error', n);
    return tn('{n} working', '{n} working', n);
  }

  function walk(lamp: LampLevel, list: SessionInfo[]): void {
    if (lamp === 'needs_input') {
      void dispatch('attention.next');
      return;
    }
    const at = list.findIndex((s) => s.id === focusedSessionId());
    const next = list[(at + 1) % list.length];
    if (next) void revealSession(next.id);
  }
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
  {#each meter as m (m.lamp)}
    <button
      type="button"
      class="seg btn"
      onclick={() => walk(m.lamp, m.list)}
      title={t('{n} {state}: go to the next one', { n: m.list.length, state: t(m.label) })}
      aria-label="{m.list.length} {t(m.label)}"
      data-testid={m.lamp === 'needs_input' ? 'status-needs-input' : `status-${m.lamp}`}
    >
      <Lamp level={m.lamp} title={t(m.label)} />
      {words(m.lamp, m.list.length)}
    </button>
  {/each}
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

  /* Full bar height so the target stays 28px. */
  .btn {
    align-self: stretch;
    padding: 0 var(--k-space-3);
    border: none;
    border-radius: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .btn:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }
</style>
