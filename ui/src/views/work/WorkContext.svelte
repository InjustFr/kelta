<script lang="ts">
  // The context bar's work item segment (#248): ticket status, phase, PR, git, cost, ⎇ details, the
  // primary action and the ⌘. work menu. The breadcrumb (key, title) and the return strip live elsewhere.
  import { t } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import { effectiveChords } from '$lib/keys/manager';
  import { sessions, settings, tickets, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { terminalPool } from '$lib/terminal';
  import { Button, currentPlatform, Icon, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { focusedSessionId } from '../../shell/nav';
  import { pluginActions, runPluginAction, workCtx } from '../plugins/actions';
  import { itemCost, overBudget, usd } from '../../shell/usage';
  import StatusChip from '../tickets/StatusChip.svelte';
  import StatusPicker from '../tickets/StatusPicker.svelte';
  import { blockedReason, runPrimary, runWorkAction, WORK_ACTIONS } from './actions';
  import { ciGlyph } from './common';
  import { phaseNow, prOf } from './live';
  import { openContent } from './nav';
  import { workUi } from './ui.svelte';

  let { projectId, workItemId }: TabHeaderProps = $props();

  const item = $derived(work.get(workItemId));
  const ticketRef = $derived(item?.ticket ?? null);
  const detail = $derived(ticketRef ? tickets.details[ticketKey(ticketRef)]?.data : null);
  const phase = $derived(item ? phaseNow(item) : null);
  const pr = $derived(item ? prOf(item) : null);
  const ci = $derived(pr ? ciGlyph(pr.ci) : null);
  const git = $derived(work.git[workItemId] ?? null);
  const spent = $derived(item ? itemCost(item, sessions.all) : 0);
  const over = $derived(overBudget(spent, settings.value(projectId)?.claude.budget_usd));
  const primaryBlocked = $derived(item && phase?.primary ? blockedReason(phase.primary, item, phase) : null);
  const menuChord = $derived(
    effectiveChords('work.menu', settings.value()?.keys ?? null, currentPlatform())[0],
  );
  // ⎇: what the bar has no room for, in its tooltip.
  const branchInfo = $derived.by(() => {
    if (!item) return '';
    const lines = [t('{branch} (vs {base})', { branch: item.branch, base: item.base }), item.worktree];
    if (item.port_base != null)
      lines.push(t('ports {from}–{to}', { from: item.port_base, to: item.port_base + 9 }));
    if (item.auto_finish) lines.push(t('armed: merges when ready, then finishes (M to disarm)'));
    if (git && !git.missing && (git.insertions || git.deletions))
      lines.push(`+${git.insertions} −${git.deletions}`);
    if (git?.dirty) lines.push(t('Uncommitted changes'));
    return lines.join('\n');
  });

  /** The status button the picker opens under; null = closed. */
  let picking = $state<HTMLElement | null>(null);
  let workBtn = $state<HTMLElement>();

  $effect(() => {
    const ref = ticketRef;
    if (ref) untrack(() => void tickets.loadDetail(ref));
  });

  // Enabled plugins' work actions (PLUGINS §4): after the built-ins, no letters.
  const plugin = $derived(pluginActions('work'));
  let pluginRunning = $state(false);

  async function runPlugin(commandId: string): Promise<void> {
    if (!item) return;
    pluginRunning = true;
    try {
      await runPluginAction(commandId, workCtx(item));
    } finally {
      pluginRunning = false;
    }
  }

  const menuItems = $derived.by((): MenuItem[] => {
    if (!item || !phase) return [];
    const out: MenuItem[] = [
      {
        id: 'primary',
        label: phase.primaryLabel || t('No next action'),
        kbd: 'enter',
        disabled: !phase.primary || primaryBlocked !== null,
        title: primaryBlocked ?? undefined,
      },
    ];
    for (const a of WORK_ACTIONS) {
      if (!a.key) continue;
      const reason = blockedReason(a.id, item, phase);
      out.push({
        id: a.id,
        label: a.label({ item, phase }),
        kbd: a.key === a.key.toLowerCase() ? a.key : `shift+${a.key.toLowerCase()}`,
        key: a.key,
        disabled: reason !== null,
        title: reason ?? undefined,
        separator: a.id === 'review_delta' || a.id === 'go_claude' || a.id === 'finish',
        danger: a.id === 'finish',
      });
    }
    plugin.forEach((a, i) =>
      out.push({ id: `plugin:${a.command_id}`, label: a.title, detail: a.plugin, separator: i === 0 }),
    );
    return out;
  });

  const menuPos = $derived.by(() => {
    if (workUi.menu !== workItemId) return null;
    const r = workBtn?.getBoundingClientRect();
    return { x: r ? r.right - 280 : 40, y: (r?.bottom ?? 40) + 2 };
  });

  function onMenu(id: string): void {
    if (!item) return;
    if (id === 'primary') void runPrimary(item);
    else if (id.startsWith('plugin:')) void runPlugin(id.slice('plugin:'.length));
    else void runWorkAction(id as (typeof WORK_ACTIONS)[number]['id'], item);
  }

  /** ⌘. is used from inside Claude or nvim: closing the menu gives the keyboard back. */
  function closeMenu(): void {
    workUi.menu = null;
    const sid = focusedSessionId();
    if (sid) terminalPool.focus(sid);
  }
</script>

{#if item && phase}
  <span
    class="work"
    role="group"
    aria-label={t('Work item')}
    data-testid="context-work"
    data-phase={phase.id}
  >
    {#if ticketRef}
      <Button
        size="sm"
        variant="secondary"
        class="ticket"
        aria-haspopup="menu"
        title={t('Move to…')}
        data-testid="work-status"
        disabled={!detail}
        onclick={(e: MouseEvent) => (picking = e.currentTarget as HTMLElement)}
      >
        {#if detail}<StatusChip status={detail.ticket.status} />{:else}…{/if}
        <Icon name="chevron-down" size={14} />
      </Button>
    {/if}
    <Button
      size="sm"
      variant="secondary"
      class="phase"
      title={t('{phase} (steps and sessions)', {
        phase: `${phase.label}${phase.detail ? `: ${phase.detail}` : ''}`,
      })}
      data-testid="work-phase"
      onclick={() =>
        void openContent(projectId, { kind: 'work_item', id: item.id }, { placement: 'split_down' })}
    >
      <Lamp level={phase.lamp} title={phase.label} />
      <span class="phase-label" class:lampless={phase.lamp === 'none'}
        >{phase.label}{#if phase.detail}<span class="detail">{phase.detail}</span>{/if}</span
      >
    </Button>
    {#if pr && ci}
      <Button
        size="sm"
        variant="ghost"
        class="pr"
        title={t('{title} ({ci}): open in the browser', { title: pr.title, ci: ci.label })}
        data-testid="work-pr"
        onclick={() => void runWorkAction('open_pr', item)}
      >
        <span class="pr-word">{t('PR')}</span><span class="num">#{pr.ref.number}</span><Lamp
          level={ci.lamp}
          title={ci.label}
        />
      </Button>
    {/if}
    {#if git && !git.missing}
      <span
        class="git num"
        title={t('{ahead} commits to push, {behind} behind {base}', {
          ahead: git.ahead,
          behind: git.behind,
          base: item.base,
        })}
      >
        <span data-testid="ahead">↑{git.ahead}</span>
        <span data-testid="behind">↓{git.behind}</span>
      </span>
    {:else if work.gitError}
      <button type="button" class="k-link" onclick={() => void work.refreshStatus()}
        >{t('status unavailable, retry')}</button
      >
    {/if}
    {#if spent > 0}
      <span class="cost num" class:over title={over ? t('Over budget') : t('Claude spend on this item')}
        >{usd(spent)}</span
      >
    {/if}
    <span class="branch" role="img" aria-label={branchInfo} title={branchInfo} data-testid="work-branch"
      ><Icon name="git-branch" size={14} />{#if item.auto_finish || git?.dirty}<span class="mark">•</span
        >{/if}</span
    >
    <span class="spacer" data-tauri-drag-region></span>
    {#if phase.primary}
      <!-- The wrapper carries the blocked reason: a disabled button shows no tooltip in WebKit. -->
      <span class="primary" title={primaryBlocked ?? (phase.id === 'failed' ? phase.detail : undefined)}>
        <Button
          size="sm"
          variant="primary"
          disabled={primaryBlocked !== null}
          data-testid="work-primary"
          onclick={() => void runPrimary(item)}
        >
          {phase.primaryLabel}
        </Button>
      </span>
    {/if}
    <span class="menu" bind:this={workBtn}>
      <Button
        size="sm"
        variant="ghost"
        icon="ellipsis"
        aria-haspopup="menu"
        aria-label={t('Work')}
        title={t('Work menu')}
        data-testid="work-menu-button"
        chord={menuChord}
        loading={pluginRunning}
        onclick={() => (workUi.menu = item.id)}>{t('Work')}</Button
      >
    </span>
  </span>
{:else}
  <span class="work muted" data-testid="context-work">{t('Work item not found')}</span>
{/if}

{#if picking && detail}
  <StatusPicker tickets={[detail.ticket]} {projectId} anchor={picking} onclose={() => (picking = null)} />
{/if}
{#if menuPos}
  <Menu
    items={menuItems}
    x={menuPos.x}
    y={menuPos.y}
    label={t('Work')}
    onselect={onMenu}
    onclose={closeMenu}
  />
{/if}

<style>
  .work {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--k-space-3);
    min-width: 0;
    height: 100%;
  }

  .work :global(.k-button) {
    flex: none;
    height: var(--k-control-height-sm);
  }

  /* The phase chip ellipsises its label before anything drops. */
  .work :global(.k-button.phase) {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
  }

  .work :global(.k-button.phase .label) {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .phase-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail {
    margin-left: var(--k-space-2);
    color: var(--k-fg-muted);
    font-weight: normal;
  }

  .work :global(.k-button.pr .label) {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .num {
    font-family: var(--k-font-mono);
    font-variant-numeric: tabular-nums;
  }

  .git,
  .cost,
  .branch {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: var(--k-space-2);
    color: var(--k-fg-muted);
  }

  .cost.over {
    color: var(--k-warn);
  }

  .mark {
    color: var(--k-warn);
  }

  .spacer {
    flex: 1;
    align-self: stretch;
  }

  .primary,
  .menu {
    display: inline-flex;
    flex: none;
  }

  /* A long primary label ("Retry …") ellipsises instead of pushing `+` out. */
  .primary :global(.k-button) {
    max-width: 28ch;
  }

  .primary :global(.k-button .label) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .muted {
    color: var(--k-fg-subtle);
  }

  /* Narrow bars drop items in this order (#248); the key, primary and `+` never drop. Steps 1 and 6
     live in ContextBar (cwd/model, the title). */
  @container contextbar (max-width: 1280px) {
    .branch {
      display: none;
    }
  }

  @container contextbar (max-width: 1180px) {
    .cost:not(.over) {
      display: none;
    }
  }

  @container contextbar (max-width: 1080px) {
    .git {
      display: none;
    }
  }

  @container contextbar (max-width: 1000px) {
    .pr-word {
      display: none;
    }
  }

  @container contextbar (max-width: 920px) {
    .work :global(.k-button.ticket) {
      display: none;
    }
  }

  @container contextbar (max-width: 760px) {
    /* A phase without a lamp (merged, closed...) has nothing else to show: keep its label. */
    .phase-label:not(.lampless) {
      display: none;
    }
  }

  @container contextbar (max-width: 700px) {
    .menu :global(.label),
    .menu :global(.chord) {
      display: none;
    }
  }
</style>
