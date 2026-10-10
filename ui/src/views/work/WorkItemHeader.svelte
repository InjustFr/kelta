<script lang="ts">
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import { workLeft } from '$lib/ipc/commands';
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
  import { claudeOf, phaseNow, workTitle } from './live';
  import { openContent } from './nav';
  import { returnBrief, workKey } from './phase';
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

  // Return strip: decided when the item is (re)focused; leaving it stamps `left_at` (no timers).
  let briefFor = $state<string | null>(null);
  let briefEl = $state<HTMLElement>();
  // The strip takes the keyboard once per return, so `x` / `v` never land in the Claude or nvim pane.
  let briefFocus = false;
  $effect(() => {
    const id = workItemId;
    untrack(() => {
      const w = work.get(id);
      const mins = settings.value()?.work.return_brief_after_mins ?? 20;
      briefFor = w && returnBrief(w, claudeOf(w)?.status ?? null, Date.now(), mins) ? id : null;
      briefFocus = briefFor !== null;
    });
    return () =>
      void workLeft({ id }).then(
        (w) => work.upsert(w),
        () => {},
      );
  });
  const brief = $derived(
    briefFor === workItemId && item && claudeOf(item)?.status !== 'working' ? item : null,
  );

  $effect(() => {
    const el = briefEl;
    if (el && briefFocus) {
      briefFocus = false;
      // After the Shell's own give-the-keyboard-back-to-the-terminal microtask (closing the switcher).
      setTimeout(() => el.focus());
    }
  });

  function dismissBrief(): void {
    briefFor = null;
    const sid = focusedSessionId();
    if (sid) terminalPool.focus(sid);
  }

  function briefKey(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey || !brief) return;
    if (e.key === 'x') dismissBrief();
    else if (e.key === 'v') void runWorkAction('review_delta', brief);
    else return;
    e.preventDefault();
  }

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
        kbd: a.key === a.key.toLowerCase() ? a.key : `shift+${a.key.toLowerCase()}`,
        key: a.key,
        disabled: reason !== null,
        title: reason ?? undefined,
        separator: a.id === 'review_delta' || a.id === 'go_claude' || a.id === 'finish',
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
    {#if item.port_base != null}
      <span class="group ports" title="KELTA_PORT … KELTA_PORT_9" data-testid="work-ports"
        >ports {item.port_base}–{item.port_base + 9}</span
      >
    {/if}
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

{#if brief}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="brief"
    role="region"
    aria-label="Where you left off"
    data-testid="return-brief"
    tabindex="-1"
    bind:this={briefEl}
    onkeydown={briefKey}
  >
    <div class="brief-text">
      {#if brief.next_note}<div class="note"><b>next:</b> {brief.next_note}</div>{/if}
      {#if brief.delta}
        <div class="since">
          <span class="ins">+{brief.delta.insertions}</span>/<span class="del">−{brief.delta.deletions}</span> since
          you reviewed
        </div>
      {/if}
      {#if brief.claude_message}
        <details>
          <summary>{brief.claude_message.split('\n')[0]}</summary>
          <pre>{brief.claude_message}</pre>
        </details>
      {/if}
    </div>
    {#if brief.delta}
      <Button
        size="sm"
        variant="secondary"
        chord="v"
        onclick={() => void runWorkAction('review_delta', brief)}>Review changes</Button
      >
    {/if}
    <Button size="sm" variant="ghost" chord="x" onclick={dismissBrief}>Dismiss</Button>
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
  .brief {
    display: flex;
    align-items: flex-start;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-4);
    border-bottom: 1px solid var(--k-border);
    background: var(--k-bg-elev);
    font-size: var(--k-font-size-sm);
  }

  .brief-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
  }

  .brief summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--k-fg-muted);
    cursor: pointer;
  }

  .brief pre {
    max-height: 30vh;
    margin: var(--k-space-1) 0 0;
    overflow: auto;
    white-space: pre-wrap;
    font: inherit;
  }

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

  .git,
  .ports {
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
    .git,
    .ports {
      display: none;
    }
  }

  @container (max-width: 820px) {
    .branch {
      display: none;
    }
  }
</style>
