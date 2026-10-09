<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { Column, TicketItem, TicketsMode } from '$lib/gen';
  import { openExternal, trackerAssign } from '$lib/ipc/commands';
  import { findPane, replacePaneContent } from '$lib/layout';
  import { layout, projects, tickets, toasts, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    Icon,
    IconButton,
    Menu,
    Tabs,
    VirtualList,
    relativeTime,
    type MenuItem,
  } from '$lib/ui';

  import { columnFor, initials, isAuthError, statusTone } from '../work/common';
  import { openContent } from '../work/nav';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { startWorkOnTicket } from '../work/startWork';
  import CommentDialog from './CommentDialog.svelte';
  import MoveDialogs from './MoveDialogs.svelte';
  import { MoveController } from './move.svelte';

  let { projectId, paneId, content, focused }: PaneProps<'tickets'> = $props();

  const ROW_HEIGHT = 34;
  const move = new MoveController();

  // ---- scope, view and mode -----------------------------------------------------------------
  const scope = $derived(content.scope);
  const project = $derived(scope.kind === 'project' ? projects.byId(scope.id) : null);
  const views = $derived(project?.tracker?.views ?? []);
  let viewOverride = $state<string | null | undefined>(undefined);
  const viewId = $derived(
    viewOverride !== undefined ? viewOverride : (content.view_id ?? views[0]?.id ?? null),
  );
  const viewLabel = $derived(views.find((v) => v.id === viewId)?.label ?? 'this view');
  let modeOverride = $state<TicketsMode | undefined>(undefined);
  const wantedMode = $derived(modeOverride ?? content.mode);
  const noTracker = $derived(project !== null && project.tracker === null);
  const mode = $derived<TicketsMode>(wantedMode === 'board' && project?.tracker ? 'board' : 'list');

  function persist(patch: { view_id?: string | null; mode?: TicketsMode }): void {
    const current = layout.get(projectId);
    if (!current || !current.tabs.some((t) => findPane(t.root, paneId))) return;
    layout.update(projectId, (l) => ({
      ...l,
      tabs: l.tabs.map((t) => ({ ...t, root: replacePaneContent(t.root, paneId, { ...content, ...patch }) })),
    }));
  }

  function setView(id: string): void {
    viewOverride = id;
    persist({ view_id: id });
  }

  function setMode(m: TicketsMode): void {
    modeOverride = m;
    persist({ mode: m });
  }

  // ---- data ---------------------------------------------------------------------------------
  const list = $derived(tickets.list(scope, viewId));
  const items = $derived(list.data?.items ?? []);
  let filter = $state('');
  const shown = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    if (needle === '') return items;
    return items.filter((i) => {
      const t = i.ticket;
      return `${t.ref.key} ${t.title} ${t.status.name} ${t.kind ?? ''} ${t.labels.join(' ')} ${t.assignee?.name ?? ''}`
        .toLowerCase()
        .includes(needle);
    });
  });

  $effect(() => {
    const s = scope;
    const v = viewId;
    if (noTracker) return;
    untrack(() => void tickets.load(s, v));
  });

  const projectIdForColumns = $derived(project?.tracker ? project.id : null);
  $effect(() => {
    const pid = projectIdForColumns;
    if (pid && mode === 'board') untrack(() => void tickets.loadColumns(pid));
  });

  const columnsState = $derived(project ? tickets.columns[project.id] : undefined);
  const columns = $derived([...(columnsState?.data ?? [])].sort((a, b) => a.order - b.order));
  const lanes = $derived.by(() => {
    const out = columns.map((column) => ({
      column: column as Column | null,
      cards: [] as TicketItem[],
    }));
    const other: TicketItem[] = [];
    for (const item of shown) {
      const col = columnFor(columns, item.ticket.status);
      const lane = col ? out.find((l) => l.column?.id === col.id) : undefined;
      if (lane) lane.cards.push(item);
      else other.push(item);
    }
    if (other.length > 0) out.push({ column: null, cards: other });
    return out;
  });

  // ---- selection ----------------------------------------------------------------------------
  const keyOf = (i: TicketItem): string => ticketKey(i.ticket.ref);
  let selKey = $state<string | null>(null);
  const cur = $derived(shown.find((i) => keyOf(i) === selKey) ?? null);

  $effect(() => {
    if (cur || shown.length === 0) return;
    const first = mode === 'board' ? lanes.find((l) => l.cards.length > 0)?.cards[0] : shown[0];
    if (first) selKey = keyOf(first);
  });

  $effect(() => {
    if (focused && cur) selectTicket(cur.ticket.ref, cur.project_ids[0] ?? projectId);
  });

  let vlist = $state<{ scrollToIndex(i: number): void }>();
  $effect(() => {
    if (!cur) return;
    if (mode === 'list') vlist?.scrollToIndex(shown.indexOf(cur));
    else rowEl(keyOf(cur))?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  });

  function rowEl(key: string): HTMLElement | undefined {
    return [...(root?.querySelectorAll<HTMLElement>('[data-key]') ?? [])].find(
      (el) => el.dataset.key === key,
    );
  }

  function step(delta: number): void {
    const seq = mode === 'board' ? (lanes.find((l) => cur && l.cards.includes(cur))?.cards ?? []) : shown;
    if (seq.length === 0) return;
    const at = cur ? seq.indexOf(cur) : -1;
    const next = seq[Math.min(seq.length - 1, Math.max(0, at + delta))];
    if (next) selKey = keyOf(next);
  }

  function jump(end: boolean): void {
    const seq = mode === 'board' ? (lanes.find((l) => cur && l.cards.includes(cur))?.cards ?? []) : shown;
    const t = end ? seq[seq.length - 1] : seq[0];
    if (t) selKey = keyOf(t);
  }

  function laneStep(delta: number): void {
    if (mode !== 'board' || !cur) return;
    const at = lanes.findIndex((l) => l.cards.includes(cur));
    const row = lanes[at]?.cards.indexOf(cur) ?? 0;
    for (let i = at + delta; i >= 0 && i < lanes.length; i += delta) {
      const cards = lanes[i]?.cards ?? [];
      if (cards.length > 0) {
        const t = cards[Math.min(row, cards.length - 1)];
        if (t) selKey = keyOf(t);
        return;
      }
    }
  }

  // ---- actions ------------------------------------------------------------------------------
  let root = $state<HTMLDivElement>();
  let filterInput = $state<HTMLInputElement>();
  let menu = $state<{ x: number; y: number; item: TicketItem; columns: Column[] } | null>(null);
  let commenting = $state<TicketItem | null>(null);

  function refresh(): void {
    void tickets.load(scope, viewId, true);
    if (projectIdForColumns && mode === 'board') void tickets.loadColumns(projectIdForColumns);
  }

  function openDetail(item: TicketItem): void {
    void openContent(projectId, { kind: 'ticket_detail', ticket: item.ticket.ref });
  }

  async function assignMe(item: TicketItem): Promise<void> {
    try {
      tickets.patch(await trackerAssign({ ticket: item.ticket.ref, assignee: { kind: 'me' } }));
      toasts.info(`${item.ticket.ref.key} assigned to you`);
    } catch (err) {
      toasts.error(err, `Assigning ${item.ticket.ref.key}`);
    }
  }

  function browse(item: TicketItem): void {
    openExternal({ url: item.ticket.url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  async function openMoveMenu(item: TicketItem): Promise<void> {
    const pid = item.project_ids[0] ?? project?.id ?? null;
    if (!pid) {
      toasts.warn(`${item.ticket.ref.key} is not bound to a project, it cannot be moved here`);
      return;
    }
    const slot = await tickets.loadColumns(pid);
    const cols = [...(slot.data ?? [])].sort((a, b) => a.order - b.order);
    if (cols.length === 0) {
      toasts.error(slot.error ?? 'No columns configured for this tracker', 'Move');
      return;
    }
    const el = rowEl(keyOf(item));
    const r = (el ?? root)?.getBoundingClientRect();
    menu = { x: (r?.left ?? 0) + 24, y: (r?.bottom ?? 0) + 2, item, columns: cols };
  }

  const menuItems = $derived<MenuItem[]>(
    menu
      ? menu.columns.map((c) => ({
          id: c.id,
          label: c.name,
          disabled: columnFor(menu!.columns, menu!.item.ticket.status)?.id === c.id,
        }))
      : [],
  );

  function menuSelect(id: string): void {
    const m = menu;
    const col = m?.columns.find((c) => c.id === id);
    if (m && col) void move.moveToColumn(m.item.ticket, col);
  }

  function shiftLane(delta: number): void {
    if (!cur || mode !== 'board') return;
    const at = columns.findIndex((c) => c.id === columnFor(columns, cur.ticket.status)?.id);
    const target = columns[at + delta];
    if (target) void move.moveToColumn(cur.ticket, target);
  }

  function onkeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, select, [role="dialog"], [role="menu"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const item = cur;
    switch (e.key) {
      case 'j':
      case 'ArrowDown':
        step(1);
        break;
      case 'k':
      case 'ArrowUp':
        step(-1);
        break;
      case 'Home':
        jump(false);
        break;
      case 'End':
        jump(true);
        break;
      case 'ArrowLeft':
        if (e.shiftKey) shiftLane(-1);
        else laneStep(-1);
        break;
      case 'ArrowRight':
        if (e.shiftKey) shiftLane(1);
        else laneStep(1);
        break;
      case 'h':
        laneStep(-1);
        break;
      case 'l':
        laneStep(1);
        break;
      case 'Enter':
        if (item) openDetail(item);
        break;
      case '/':
        filterInput?.focus();
        filterInput?.select();
        break;
      case 'R':
        refresh();
        break;
      case 'm':
        if (item) void openMoveMenu(item);
        break;
      case 'a':
        if (item) void assignMe(item);
        break;
      case 'c':
        if (item) commenting = item;
        break;
      case 'o':
        if (item) browse(item);
        break;
      case 's':
        if (item) void startWorkOnTicket(item.ticket.ref, item.project_ids[0] ?? projectId);
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  function filterKeys(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      filter = '';
      root?.focus();
      e.stopPropagation();
    } else if (e.key === 'Enter' || e.key === 'ArrowDown') {
      root?.focus();
      e.preventDefault();
    }
  }

  $effect(() => {
    if (focused && root && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
  });

  // ---- drag and drop (board) ----------------------------------------------------------------
  let dragKey = $state<string | null>(null);
  let overLane = $state<string | null>(null);

  function dragStart(e: DragEvent, item: TicketItem): void {
    dragKey = keyOf(item);
    selKey = dragKey;
    e.dataTransfer?.setData('text/plain', dragKey);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
  }

  function drop(e: DragEvent, column: Column | null): void {
    e.preventDefault();
    const key = dragKey ?? e.dataTransfer?.getData('text/plain') ?? null;
    dragKey = null;
    overLane = null;
    const item = items.find((i) => keyOf(i) === key);
    if (!item || !column) return;
    if (columnFor(columns, item.ticket.status)?.id === column.id) return;
    void move.moveToColumn(item.ticket, column);
  }

  const hasWork = (item: TicketItem): boolean =>
    item.work_item_id !== null || work.forTicket(item.ticket.ref) !== null;
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane"
  data-testid="tickets-pane"
  data-mode={mode}
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Tickets"
  {onkeydown}
>
  <header class="bar">
    {#if views.length > 1}
      <Tabs
        label="Views"
        items={views.map((v) => ({ id: v.id, label: v.label }))}
        value={viewId ?? undefined}
        onchange={setView}
      />
    {:else}
      <span class="title">{views[0]?.label ?? (scope.kind === 'all' ? 'All projects' : 'Tickets')}</span>
    {/if}
    <span class="spacer"></span>
    <input
      class="filter"
      bind:this={filterInput}
      bind:value={filter}
      placeholder="Filter (/)"
      aria-label="Filter tickets"
      onkeydown={filterKeys}
    />
    {#if project?.tracker}
      <Tabs
        label="Display mode"
        items={[
          { id: 'list', label: 'List', icon: 'list' },
          { id: 'board', label: 'Board', icon: 'kanban' },
        ]}
        value={mode}
        onchange={setMode}
      />
    {/if}
    <IconButton icon="refresh-cw" label="Refresh (R)" size="sm" onclick={refresh} />
  </header>

  {#if noTracker}
    <EmptyState
      icon="ticket"
      title="No tracker bound"
      body={`${project?.name ?? 'This project'} has no tracker. Bind one to see your tickets.`}
    >
      {#snippet actions()}
        <Button variant="primary" onclick={() => void dispatch('settings.open', { section: 'projects' })}>
          Bind a tracker
        </Button>
      {/snippet}
    </EmptyState>
  {:else if list.loading && !list.data}
    <Loading label="Loading tickets" />
  {:else if !list.data && list.error}
    <ErrorState error={list.error} title="Could not load tickets" onretry={refresh}>
      {#snippet actions()}
        {#if isAuthError(list.error)}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}
            >Re-authenticate</Button
          >
        {:else}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}
            >Open settings</Button
          >
        {/if}
      {/snippet}
    </ErrorState>
  {:else}
    <StateBanner
      stale={list.stale}
      fetchedAt={list.fetchedAt}
      error={list.data ? list.error : null}
      errors={list.data?.errors ?? []}
      onretry={refresh}
    />
    {#if items.length === 0}
      <EmptyState
        icon="ticket"
        title={`Nothing assigned to you in ${viewLabel}`}
        body="Switch view or refresh."
      >
        {#snippet actions()}
          {#if views.length > 1}
            <Button
              onclick={() => {
                const next = views[(views.findIndex((v) => v.id === viewId) + 1) % views.length];
                if (next) setView(next.id);
              }}>Switch view</Button
            >
          {/if}
          <Button onclick={refresh}>Refresh</Button>
        {/snippet}
      </EmptyState>
    {:else if shown.length === 0}
      <EmptyState icon="search" title="No ticket matches the filter" body={`Nothing matches "${filter}".`}>
        {#snippet actions()}<Button onclick={() => (filter = '')}>Clear filter</Button>{/snippet}
      </EmptyState>
    {:else if mode === 'list'}
      <div class="list">
        <VirtualList
          bind:this={vlist}
          items={shown}
          itemHeight={ROW_HEIGHT}
          key={(i) => keyOf(i)}
          label="Tickets"
          onend={() =>
            void tickets.loadMore(scope, viewId).catch((e) => toasts.error(e, 'Loading more tickets'))}
        >
          {#snippet row(item)}
            <button
              type="button"
              tabindex="-1"
              class="row"
              class:selected={cur === item}
              aria-current={cur === item ? 'true' : undefined}
              data-key={keyOf(item)}
              onclick={() => (selKey = keyOf(item))}
              ondblclick={() => openDetail(item)}
            >
              <span class="key">{item.ticket.ref.key}</span>
              <span class="ttl">{item.ticket.title}</span>
              {#if hasWork(item)}<Badge tone="accent" title="Local work in progress">work</Badge>{/if}
              {#if scope.kind === 'all'}
                <Badge>{item.project_ids[0] ?? 'Other'}</Badge>
              {/if}
              {#each item.ticket.labels.slice(0, 2) as l (l)}<Badge>{l}</Badge>{/each}
              {#if item.ticket.priority}<span class="prio">{item.ticket.priority}</span>{/if}
              <Badge tone={statusTone(item.ticket.status.category)}>{item.ticket.status.name}</Badge>
              <span class="who" title={item.ticket.assignee?.name ?? 'Unassigned'}>
                {item.ticket.assignee ? initials(item.ticket.assignee.name) : '–'}
              </span>
              <span class="age">{relativeTime(Date.parse(item.ticket.updated_at))}</span>
            </button>
          {/snippet}
        </VirtualList>
      </div>
    {:else}
      <div class="board" data-testid="board">
        {#each lanes as lane (lane.column?.id ?? '__other')}
          <section
            class="lane"
            class:over={overLane === (lane.column?.id ?? '__other')}
            aria-label={`${lane.column?.name ?? 'Other'} column`}
            data-column={lane.column?.id ?? '__other'}
            role="group"
            ondragover={(e) => {
              if (lane.column) {
                e.preventDefault();
                overLane = lane.column.id;
              }
            }}
            ondragleave={() => (overLane = null)}
            ondrop={(e) => drop(e, lane.column)}
          >
            <h3>{lane.column?.name ?? 'Other'} <Badge>{lane.cards.length}</Badge></h3>
            <div class="cards">
              {#each lane.cards as item (keyOf(item))}
                <button
                  type="button"
                  tabindex="-1"
                  class="card"
                  class:selected={cur === item}
                  aria-current={cur === item ? 'true' : undefined}
                  draggable="true"
                  data-key={keyOf(item)}
                  ondragstart={(e) => dragStart(e, item)}
                  ondragend={() => {
                    dragKey = null;
                    overLane = null;
                  }}
                  onclick={() => (selKey = keyOf(item))}
                  ondblclick={() => openDetail(item)}
                >
                  <span class="card-top">
                    <span class="key">{item.ticket.ref.key}</span>
                    {#if hasWork(item)}<Badge tone="accent" title="Local work in progress">work</Badge>{/if}
                    <span class="who">{item.ticket.assignee ? initials(item.ticket.assignee.name) : '–'}</span
                    >
                  </span>
                  <span class="card-title">{item.ticket.title}</span>
                  {#if item.ticket.labels.length > 0 || item.ticket.priority}
                    <span class="card-meta">
                      {#if item.ticket.priority}<span class="prio">{item.ticket.priority}</span>{/if}
                      {#each item.ticket.labels.slice(0, 2) as l (l)}<Badge>{l}</Badge>{/each}
                    </span>
                  {/if}
                </button>
              {/each}
            </div>
          </section>
        {/each}
        {#if columnsState?.loading && columns.length === 0}
          <Loading label="Loading columns" />
        {:else if columnsState?.error && columns.length === 0}
          <ErrorState error={columnsState.error} title="Could not load columns" onretry={refresh} />
        {/if}
        {#if list.data?.next}
          <Button
            size="sm"
            loading={list.loadingMore}
            onclick={() =>
              void tickets.loadMore(scope, viewId).catch((e) => toasts.error(e, 'Loading more tickets'))}
          >
            <Icon name="plus" size={12} /> More
          </Button>
        {/if}
      </div>
    {/if}
  {/if}

  <footer class="hints" aria-hidden="true">
    j/k move · Enter open · / filter · m move · a assign me · c comment · o browser · s start work · R refresh
  </footer>
</div>

{#if menu}
  <Menu
    items={menuItems}
    x={menu.x}
    y={menu.y}
    label="Move to"
    onselect={menuSelect}
    onclose={() => {
      menu = null;
      root?.focus();
    }}
  />
{/if}
{#if commenting}
  <CommentDialog
    ticket={commenting.ticket.ref}
    onclose={() => {
      commenting = null;
      root?.focus();
    }}
  />
{/if}
<MoveDialogs {move} />

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    outline: none;
    background: var(--k-bg);
    color: var(--k-fg);
  }

  .pane:focus-visible {
    box-shadow: inset 0 0 0 1px var(--k-focus);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
  }

  .title {
    font-weight: 600;
  }

  .spacer {
    flex: 1;
  }

  .filter {
    width: 180px;
    height: 24px;
    padding: 0 var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    color: var(--k-fg);
    font: inherit;
  }

  .list {
    flex: 1;
    min-height: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    width: 100%;
    height: 100%;
    padding: 0 var(--k-space-3);
    border: 0;
    border-bottom: 1px solid var(--k-border);
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--k-bg-hover);
  }

  .selected {
    background: var(--k-bg-selected);
  }

  .key {
    flex: none;
    min-width: 72px;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .ttl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .prio,
  .age {
    flex: none;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .who {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--k-bg-sunken);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .board {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: var(--k-space-3);
    padding: var(--k-space-3);
    overflow: auto;
  }

  .lane {
    flex: 0 0 260px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg-sunken);
  }

  .lane.over {
    border-color: var(--k-accent);
  }

  .lane h3 {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    font-size: var(--k-font-size-sm);
  }

  .cards {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: var(--k-space-2);
    overflow: auto;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    padding: var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-elev);
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: grab;
  }

  .card.selected {
    border-color: var(--k-accent);
    background: var(--k-bg-selected);
  }

  .card-top,
  .card-meta {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .card-top .key {
    flex: 1;
    min-width: 0;
  }

  .card-title {
    overflow-wrap: anywhere;
  }

  .hints {
    padding: var(--k-space-1) var(--k-space-3);
    border-top: 1px solid var(--k-border);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }
</style>
