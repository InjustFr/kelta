<script lang="ts">
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import { effectiveChords } from '$lib/keys/manager';
  import { settings, tickets, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { terminalPool } from '$lib/terminal';
  import { Badge, Button, currentPlatform, Icon, Kbd, Menu, type MenuItem } from '$lib/ui';

  import AttentionDot from '../../shell/AttentionDot.svelte';
  import { focusedSessionId } from '../../shell/nav';
  import MoveDialogs from '../tickets/MoveDialogs.svelte';
  import { MoveController } from '../tickets/move.svelte';
  import { blockedReason, runPrimary, runWorkAction, WORK_ACTIONS } from './actions';
  import { statusTone } from './common';
  import CreatePrDialog from './CreatePrDialog.svelte';
  import FinishDialog from './FinishDialog.svelte';
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
  const primaryBlocked = $derived(item && phase?.primary ? blockedReason(phase.primary, item, phase) : null);
  const menuChord = $derived(
    effectiveChords('work.menu', settings.value()?.keys ?? null, currentPlatform())[0],
  );

  let statusMenu = $state<{ x: number; y: number } | null>(null);
  let statusBtn = $state<HTMLElement>();
  let workBtn = $state<HTMLElement>();

  // Git status is read for every item at once (startup, window focus, Now); a header without one
  // asks, and so does a Claude stop or a state change of this item.
  $effect(() => {
    void item?.state.kind;
    void item?.review_due;
    void item?.claude_replied;
    untrack(() => {
      if (item && item.state.kind !== 'finished') void work.refreshStatus();
    });
  });

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

  function openStatusMenu(): void {
    const r = statusBtn?.getBoundingClientRect();
    statusMenu = { x: r?.left ?? 40, y: (r?.bottom ?? 40) + 2 };
  }

  function statusSelect(id: string): void {
    const t = transitions.find((x) => x.id === id);
    if (t && detail) void move.moveViaTransition(detail.ticket, t);
  }
</script>

{#if item && phase}
  <div class="bar" role="toolbar" aria-label="Work item" data-testid="work-header" data-phase={phase.id}>
    <span class="lamp"><AttentionDot level={phase.lamp} size={8} title={phase.label} /></span>
    <span class="key">{workKey(item)}</span>
    <span class="ttl" title={workTitle(item)}>{workTitle(item)}</span>
    {#if ticketRef}
      <button
        type="button"
        class="status"
        bind:this={statusBtn}
        aria-haspopup="menu"
        title="Move to…"
        disabled={!detail}
        onclick={openStatusMenu}
      >
        <Badge tone={statusTone(detail?.ticket.status.category ?? 'unknown')}>
          {detail?.ticket.status.name ?? '…'}
        </Badge>
        <Icon name="chevron-down" size={12} />
      </button>
    {/if}
    <button
      type="button"
      class="phase"
      title="Steps and sessions"
      data-testid="work-phase"
      onclick={() =>
        void openContent(projectId, { kind: 'work_item', id: item.id }, { placement: 'split_down' })}
    >
      <span class="label">{phase.label}</span>
      {#if phase.detail}<span class="detail">{phase.detail}</span>{/if}
    </button>
    <span class="branch" title={item.worktree}>
      <Icon name="git-branch" size={12} />
      <code>{item.branch}</code>
    </span>
    {#if git && !git.missing}
      <span
        class="num"
        title={`${git.ahead} ahead, ${git.behind} behind ${item.base}, ${git.files} files changed since the merge base`}
      >
        <span data-testid="ahead">↑{git.ahead}</span>
        <span data-testid="behind">↓{git.behind}</span>
        {#if git.insertions || git.deletions}
          <span class="ins">+{git.insertions}</span><span class="del">−{git.deletions}</span>
        {/if}
      </span>
      {#if git.dirty}<span class="tag" title="Part of the diff is uncommitted">dirty</span>{/if}
    {:else if work.gitError}
      <button type="button" class="link" onclick={() => void work.refreshStatus()}
        >status unavailable, retry</button
      >
    {/if}
    <span class="spacer"></span>
    {#if phase.primary}
      <Button
        size="sm"
        variant="primary"
        disabled={primaryBlocked !== null}
        title={primaryBlocked ?? undefined}
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
        onclick={() => (workUi.menu = item.id)}
      >
        Work {#if menuChord}<Kbd chord={menuChord} />{/if}
      </Button>
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
{#if workUi.ship === workItemId && item}
  <CreatePrDialog {item} onclose={() => (workUi.ship = null)} />
{/if}
{#if workUi.finish === workItemId && item}
  <FinishDialog {item} onclose={() => (workUi.finish = null)} />
{/if}
<MoveDialogs {move} />

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    min-height: 30px;
    padding: 0 var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
    background: var(--k-bg-elev);
    font-size: var(--k-font-size-sm);
    white-space: nowrap;
  }

  .lamp {
    display: inline-flex;
    justify-content: center;
    flex: none;
    width: 10px;
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
    min-width: 0;
    max-width: 32ch;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
  }

  .phase {
    display: inline-flex;
    align-items: baseline;
    gap: var(--k-space-2);
    min-width: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-fg);
    font: inherit;
    cursor: pointer;
  }

  .phase:hover .label {
    text-decoration: underline;
  }

  .phase .detail {
    max-width: 40ch;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--k-fg-muted);
  }

  .branch {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    min-width: 0;
    overflow: hidden;
    color: var(--k-fg-muted);
  }

  .num {
    display: inline-flex;
    gap: var(--k-space-2);
    color: var(--k-fg-muted);
  }

  .ins {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }

  .tag {
    padding: 0 var(--k-space-1);
    border-radius: 2px;
    background: var(--k-bg-sunken);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
  }

  .spacer {
    flex: 1;
  }

  .status,
  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-fg-muted);
    font: inherit;
    cursor: pointer;
  }

  .link {
    color: var(--k-accent);
  }

  .muted {
    color: var(--k-fg-subtle);
  }
</style>
