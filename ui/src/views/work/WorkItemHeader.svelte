<script lang="ts">
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import { effectiveChords } from '$lib/keys/manager';
  import { sessions, settings, tickets, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { terminalPool } from '$lib/terminal';
  import { Button, currentPlatform, Icon, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { focusedSessionId } from '../../shell/nav';
  import { itemCost, overBudget, usd } from '../../shell/usage';
  import MoveDialogs from '../tickets/MoveDialogs.svelte';
  import { MoveController } from '../tickets/move.svelte';
  import { blockedReason, runPrimary, runWorkAction, WORK_ACTIONS } from './actions';
  import { phaseNow, workTitle } from './live';
  import { openContent } from './nav';
  import { workKey } from './phase';
  import { workUi } from './ui.svelte';

  let { projectId, workItemId }: TabHeaderProps = $props();

  const move = new MoveController();
  const item = $derived(work.get(workItemId));
  const ticketRef = $derived(item?.ticket ?? null);
  const detail = $derived(ticketRef ? tickets.details[ticketKey(ticketRef)]?.data : null);
  const transitions = $derived(ticketRef ? (tickets.transitions[ticketKey(ticketRef)]?.data ?? []) : []);
  const phase = $derived(item ? phaseNow(item) : null);
  const git = $derived(work.git[workItemId] ?? null);
  const spent = $derived(item ? itemCost(item, sessions.all) : 0);
  const over = $derived(overBudget(spent, settings.value(projectId)?.claude.budget_usd));
  const primaryBlocked = $derived(item && phase?.primary ? blockedReason(phase.primary, item, phase) : null);
  const menuChord = $derived(
    effectiveChords('work.menu', settings.value()?.keys ?? null, currentPlatform())[0],
  );

  let statusMenu = $state<{ x: number; y: number } | null>(null);
  let workBtn = $state<HTMLElement>();

  $effect(() => {
    const ref = ticketRef;
    if (ref) {
      untrack(() => {
        void tickets.loadDetail(ref);
        void tickets.loadTransitions(ref);
      });
    }
  });

  const statusItems = $derived<MenuItem[]>(
    transitions.map((t) => ({
      id: t.id,
      label: t.name === t.to.name ? t.name : `${t.name} → ${t.to.name}`,
      disabled: detail?.ticket.status.name === t.to.name,
    })),
  );

  const menuItems = $derived.by((): MenuItem[] => {
    if (!item || !phase) return [];
    const out: MenuItem[] = [
      {
        id: 'primary',
        label: phase.primaryLabel || 'No next action',
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
        kbd: a.key === 'F' ? 'shift+f' : a.key,
        key: a.key,
        disabled: reason !== null,
        title: reason ?? undefined,
        separator: a.id === 'review_diff' || a.id === 'go_claude' || a.id === 'finish',
        danger: a.id === 'finish',
      });
    }
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
    else void runWorkAction(id as (typeof WORK_ACTIONS)[number]['id'], item);
  }

  /** ⌘. is used from inside Claude or nvim: closing the menu gives the keyboard back. */
  function closeMenu(): void {
    workUi.menu = null;
    const sid = focusedSessionId();
    if (sid) terminalPool.focus(sid);
  }

  function openStatusMenu(e: MouseEvent): void {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    statusMenu = { x: r.left, y: r.bottom + 2 };
  }

  function statusSelect(id: string): void {
    const t = transitions.find((x) => x.id === id);
    if (t && detail) void move.moveViaTransition(detail.ticket, t);
  }
</script>

{#if item && phase}
  <div class="bar" role="toolbar" aria-label="Work item" data-testid="work-header" data-phase={phase.id}>
    <span class="group identity">
      <span class="lamp"><Lamp level={phase.lamp} title={phase.label} /></span>
      <span class="key" data-testid="work-key">{workKey(item)}</span>
      <span class="ttl" title={workTitle(item)}>{workTitle(item)}</span>
    </span>
    <span class="group state">
      {#if ticketRef}
        <Button
          size="sm"
          variant="secondary"
          aria-haspopup="menu"
          title="Move to…"
          disabled={!detail}
          onclick={openStatusMenu}
        >
          {detail?.ticket.status.name ?? '…'}
          <Icon name="chevron-down" size={14} />
        </Button>
      {/if}
      <Button
        size="sm"
        variant="secondary"
        title="Steps and sessions"
        data-testid="work-phase"
        onclick={() =>
          void openContent(projectId, { kind: 'work_item', id: item.id }, { placement: 'split_down' })}
      >
        Steps: {phase.label}
        {#if phase.detail}<span class="detail">{phase.detail}</span>{/if}
      </Button>
    </span>
    <span class="group branch" title={item.worktree}>
      <Icon name="git-branch" size={14} />
      <code>{item.branch}</code>
    </span>
    {#if git && !git.missing}
      <span class="group git" title="{git.ahead} commits to push, {git.behind} behind {item.base}">
        <span class="num">
          <span data-testid="ahead">↑{git.ahead}</span>
          <span data-testid="behind">↓{git.behind}</span>
        </span>
        <span class="vs">vs {item.base}</span>
        {#if git.insertions || git.deletions}
          <span class="num"
            ><span class="ins">+{git.insertions}</span><span class="del">−{git.deletions}</span></span
          >
        {/if}
        {#if git.dirty}<span class="tag" title="Part of the diff is uncommitted">Uncommitted changes</span
          >{/if}
      </span>
    {:else if work.gitError}
      <button type="button" class="link" onclick={() => void work.refreshStatus()}
        >status unavailable, retry</button
      >
    {/if}
    {#if spent > 0}
      <span class="group cost" class:over title={over ? 'Over budget' : 'Claude spend on this item'}
        >{usd(spent)}</span
      >
    {/if}
    <span class="spacer"></span>
    <span class="group actions">
      {#if phase.primary}
        {#if primaryBlocked}<span class="blocked">{primaryBlocked}</span>{/if}
        <Button
          size="sm"
          variant="primary"
          disabled={primaryBlocked !== null}
          title={primaryBlocked ?? (phase.id === 'failed' ? phase.detail : undefined)}
          onclick={() => void runPrimary(item)}
        >
          {phase.primaryLabel}
        </Button>
      {/if}
      <span bind:this={workBtn}>
        <Button
          size="sm"
          variant="ghost"
          aria-haspopup="menu"
          title="Work menu"
          data-testid="work-menu-button"
          chord={menuChord}
          onclick={() => (workUi.menu = item.id)}>Work</Button
        >
      </span>
    </span>
  </div>
{:else}
  <div class="bar" data-testid="work-header">
    <span class="muted">Work item not found</span>
  </div>
{/if}

{#if statusMenu}
  <Menu
    items={statusItems}
    x={statusMenu.x}
    y={statusMenu.y}
    label="Move to"
    onselect={statusSelect}
    onclose={() => (statusMenu = null)}
  />
{/if}
{#if menuPos}
  <Menu items={menuItems} x={menuPos.x} y={menuPos.y} label="Work" onselect={onMenu} onclose={closeMenu} />
{/if}
<MoveDialogs {move} />

<style>
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: nowrap;
    gap: var(--k-space-3);
    height: var(--k-work-header-height);
    padding: 0 var(--k-space-4);
    container-type: inline-size;
    background: var(--k-bezel-raised);
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-sm);
    white-space: nowrap;
  }

  .group {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    min-width: 0;
  }

  /* Identity, state and actions lay their parts straight into the bar, so each part shrinks with its
     own floor (title, steps, blocked reason) instead of a group overflowing onto its neighbour. */
  .identity,
  .state,
  .actions {
    display: contents;
  }

  .key {
    flex: none;
  }

  .lamp {
    display: inline-flex;
    justify-content: center;
    flex: none;
    width: 12px;
  }

  .key,
  .branch code,
  .num {
    font-family: var(--k-font-mono);
    font-variant-numeric: tabular-nums;
  }

  .key {
    color: var(--k-fg-muted);
  }

  .ttl {
    flex: 0 1 auto;
    min-width: 16ch;
    max-width: 32ch;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
  }

  /* The status keeps its width; the steps button ellipsises down to a readable "Steps…". */
  .bar :global(.k-button) {
    flex: none;
  }

  .bar :global(.k-button[data-testid='work-phase']) {
    flex: 0 1 auto;
    min-width: 9ch;
    overflow: hidden;
  }

  .bar :global(.k-button[data-testid='work-phase'] .label) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail {
    margin-left: var(--k-space-2);
    color: var(--k-fg-muted);
    font-weight: normal;
  }

  .branch {
    flex: 0 1 auto;
    min-width: 8ch;
    gap: var(--k-space-2);
    overflow: hidden;
    color: var(--k-fg-muted);
  }

  .branch code {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .git {
    flex: none;
    color: var(--k-fg-muted);
  }

  .cost {
    flex: none;
    color: var(--k-fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .cost.over {
    color: var(--k-danger);
  }

  .num {
    display: inline-flex;
    gap: var(--k-space-2);
  }

  .ins {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }

  .tag {
    padding: 0 6px;
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    line-height: 20px;
  }

  /* A long primary label ("Retry …") gives way before the branch does. */
  .actions :global(.k-button) {
    max-width: 28ch;
  }

  .actions :global(.k-button .label) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .blocked {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 32ch;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--k-fg-muted);
  }

  .spacer {
    flex: 1;
  }

  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-accent);
    font: inherit;
    cursor: pointer;
  }

  .muted {
    color: var(--k-fg-subtle);
  }

  /* Narrow bars drop detail in order of value: phase detail and base, then git, then the branch. */
  @container (max-width: 1240px) {
    .vs,
    .detail {
      display: none;
    }
  }

  @container (max-width: 1000px) {
    .git {
      display: none;
    }
  }

  @container (max-width: 820px) {
    .branch {
      display: none;
    }
  }
</style>
