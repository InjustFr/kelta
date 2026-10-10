<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { Column, TicketItem, TicketsMode, Transition, Who } from '$lib/gen';
  import { openExternal, trackerAssign } from '$lib/ipc/commands';
  import { findPane, replacePaneContent } from '$lib/layout';
  import { layout, projects, tickets, toasts, ui, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import type { SheetKey } from '$lib/stores/ui.svelte';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    Icon,
    IconButton,
    Lamp,
    Menu,
    Select,
    Tabs,
    VirtualList,
    relativeTime,
    type MenuItem,
  } from '$lib/ui';

  import { columnFor, initials, isAuthError } from '../work/common';
  import { sessionsLamp } from '../../shell/nav';
  import { openContent } from '../work/nav';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { startWorkOnTicket } from '../work/startWork';
  import CommentDialog from './CommentDialog.svelte';
  import MoveDialogs from './MoveDialogs.svelte';
  import MoveMenu from './MoveMenu.svelte';
  import StatusChip from './StatusChip.svelte';
  import { GROUP_BYS, groupTickets, type GroupBy, type TicketGroup } from './group';
  import { MoveController } from './move.svelte';
  import { ciLamp, ensureReviews, loadedReviews, prForTicket, prLabel } from './prLink';

  let { projectId, paneId, content, focused }: PaneProps<'tickets'> = $props();

  const ROW_HEIGHT = 26;
  const WHOS: readonly Who[] = ['mine', 'unassigned', 'anyone'];
  const WHO_LABELS: Record<Who, string> = { mine: 'Mine', unassigned: 'Unassigned', anyone: 'Anyone' };
  const GROUP_LABELS: Record<GroupBy, string> = {
    status: 'Group by status',
    assignee: 'Group by assignee',
    source: 'Group by source',
    none: 'No grouping',
  };
  const move = new MoveController();

  // ---- scope, source, who and mode ----------------------------------------------------------
  const scope = $derived(content.scope);
  const project = $derived(scope.kind === 'project' ? projects.byId(scope.id) : null);
  const views = $derived(project?.tracker?.views ?? []);
  const accountOf = (v: (typeof views)[number]): string => v.account ?? project?.tracker?.account ?? '';
  const multiAccount = $derived(new Set(views.map(accountOf)).size > 1);
  let viewOverride = $state<string | null | undefined>(undefined);
  /** `null` = all sources (the union of the project's views). */
  const viewId = $derived(viewOverride !== undefined ? viewOverride : content.view_id);
  const sourceLabel = $derived(
    viewId ? (views.find((v) => v.id === viewId)?.label ?? 'this source') : (project?.name ?? 'all projects'),
  );
  const noSource = $derived(project !== null && views.length === 0);

  let whoOverride = $state<Who | null | undefined>(undefined);
  /** The persisted who the list loads with (`null` = each view decides). */
  const who = $derived(whoOverride !== undefined ? whoOverride : content.who);
  /** The tab shown: the pane's who, else the who every queried view agrees on. */
  const whoTab = $derived.by<Who | null>(() => {
    if (who) return who;
    const ws = new Set((viewId ? views.filter((v) => v.id === viewId) : views).map((v) => v.who));
    return ws.size === 1 ? ([...ws][0] ?? null) : null;
  });

  let modeOverride = $state<TicketsMode | undefined>(undefined);
  const wantedMode = $derived(modeOverride ?? content.mode);
  const mode = $derived<TicketsMode>(wantedMode === 'board' && project?.tracker ? 'board' : 'list');
  // shortcut: grouping is per pane instance (PaneContent has no field), upgrade = a `group` field in the proto.
  let groupBy = $state<GroupBy>('status');

  function persist(patch: { view_id?: string | null; mode?: TicketsMode; who?: Who | null }): void {
    const current = layout.get(projectId);
    if (!current || !current.tabs.some((t) => findPane(t.root, paneId))) return;
    layout.update(projectId, (l) => ({
      ...l,
      tabs: l.tabs.map((t) => ({ ...t, root: replacePaneContent(t.root, paneId, { ...content, ...patch }) })),
    }));
  }

  function setView(id: string | null): void {
    viewOverride = id;
    persist({ view_id: id });
  }

  function setWho(w: Who): void {
    whoOverride = w;
    persist({ who: w });
  }

  function setMode(m: TicketsMode): void {
    modeOverride = m;
    persist({ mode: m });
  }

  function addSource(): void {
    // shortcut: the key is WP4's sheet, not yet in SheetKey; drop the cast once it is registered.
    ui.openSheet('tracker.source_picker' as SheetKey, { projectId: project?.id ?? projectId });
  }

  // ---- data ---------------------------------------------------------------------------------
  const list = $derived(tickets.list(scope, viewId, who));
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
    const [s, v, w, tab] = [scope, viewId, who, whoTab];
    if (noSource) return;
    untrack(() => {
      void tickets.load(s, v, false, w);
      // Counts for the other tabs (served from the core's cache); Anyone can be large, so only on demand.
      for (const o of ['mine', 'unassigned'] as const) {
        const l = tickets.list(s, v, o);
        if (o !== tab && !l.data && !l.loading) void tickets.load(s, v, false, o);
      }
      ensureReviews();
    });
  });

  const countOf = (w: Who): number | undefined =>
    (w === whoTab ? list : tickets.list(scope, viewId, w)).data?.items.length;

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

  // ---- grouped rows (list) ------------------------------------------------------------------
  type Row =
    | { kind: 'group'; key: string; group: TicketGroup; open: boolean }
    | { kind: 'ticket'; key: string; item: TicketItem };

  const keyOf = (i: TicketItem): string => ticketKey(i.ticket.ref);
  /** Groups the user opened or closed (`groupBy:id`); Done groups start closed. */
  let toggled = $state<Record<string, boolean>>({});
  // shortcut: no Source grouping across all projects (views are per project), add project groups if asked.
  const groupBys = $derived(scope.kind === 'all' ? GROUP_BYS.filter((g) => g !== 'source') : GROUP_BYS);
  const groups = $derived(groupTickets(shown, groupBy, views));
  const rows = $derived.by<Row[]>(() => {
    if (groupBy === 'none') return shown.map((item) => ({ kind: 'ticket', key: keyOf(item), item }));
    const out: Row[] = [];
    for (const group of groups) {
      const open = toggled[`${groupBy}:${group.id}`] ?? group.category !== 'done';
      out.push({ kind: 'group', key: `group:${group.id}`, group, open });
      if (open) for (const item of group.items) out.push({ kind: 'ticket', key: keyOf(item), item });
    }
    return out;
  });

  function toggleGroup(row: Row): void {
    if (row.kind !== 'group') return;
    toggled = { ...toggled, [`${groupBy}:${row.group.id}`]: !row.open };
  }

  const showSource = $derived(
    groupBy !== 'source' && (scope.kind === 'all' || (views.length > 1 && viewId === null)),
  );
  const sourceOf = (item: TicketItem): string =>
    scope.kind === 'all'
      ? (projects.byId(item.project_ids[0] ?? '')?.name ?? 'Other')
      : (views.find((v) => v.id === item.view_ids[0])?.label ?? 'Other');

  const reviewPool = $derived(loadedReviews());
  const workOf = (item: TicketItem) =>
    work.forTicket(item.ticket.ref) ?? (item.work_item_id ? work.get(item.work_item_id) : null);
  const prOf = (item: TicketItem) => prForTicket(item.ticket, workOf(item)?.pr_url ?? null, reviewPool);

  // ---- selection ----------------------------------------------------------------------------
  let selKey = $state<string | null>(null);
  const selRow = $derived(mode === 'list' ? (rows.find((r) => r.key === selKey) ?? null) : null);
  const cur = $derived(
    mode === 'list'
      ? selRow?.kind === 'ticket'
        ? selRow.item
        : null
      : (shown.find((i) => keyOf(i) === selKey) ?? null),
  );

  /** The selected row's last index: a row that leaves the list (moved to Done) hands over to its neighbour. */
  let lastIndex: number | null = null;
  $effect(() => {
    if ((mode === 'list' ? selRow : cur) || shown.length === 0) return;
    const first =
      mode === 'board'
        ? lanes.find((l) => l.cards.length > 0)?.cards[0]
        : lastIndex !== null
          ? rows[Math.min(lastIndex, rows.length - 1)]
          : (rows.find((r) => r.kind === 'ticket') ?? rows[0]);
    if (first) selKey = 'key' in first ? first.key : keyOf(first);
  });

  $effect(() => {
    if (!focused || !cur) return;
    selectTicket(cur.ticket.ref, cur.project_ids[0] ?? projectId);
    return () => selectTicket(null, null);
  });

  let vlist = $state<{ scrollToIndex(i: number): void }>();
  $effect(() => {
    if (mode === 'list') {
      if (selRow) vlist?.scrollToIndex((lastIndex = rows.indexOf(selRow)));
    } else if (cur) rowEl(keyOf(cur))?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  });

  function rowEl(key: string): HTMLElement | undefined {
    return [...(root?.querySelectorAll<HTMLElement>('[data-key]') ?? [])].find(
      (el) => el.dataset.key === key,
    );
  }

  /** Keys of the rows j/k walk: the grouped rows, or the current lane's cards. */
  function sequence(): string[] {
    if (mode === 'list') return rows.map((r) => r.key);
    return (lanes.find((l) => cur && l.cards.includes(cur))?.cards ?? []).map(keyOf);
  }

  function step(delta: number): void {
    const seq = sequence();
    if (seq.length === 0) return;
    const at = selKey ? seq.indexOf(selKey) : -1;
    selKey = seq[Math.min(seq.length - 1, Math.max(0, at + delta))] ?? selKey;
  }

  function jump(end: boolean): void {
    const seq = sequence();
    selKey = (end ? seq[seq.length - 1] : seq[0]) ?? selKey;
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
  let sourceBtn = $state<HTMLElement>();
  let moveMenu = $state<{ x: number; y: number; item: TicketItem; transitions: Transition[] | null } | null>(
    null,
  );
  let sourceMenu = $state<{ x: number; y: number } | null>(null);
  let commenting = $state<TicketItem | null>(null);

  function refresh(): void {
    void tickets.load(scope, viewId, true, who);
    if (projectIdForColumns && mode === 'board') void tickets.loadColumns(projectIdForColumns);
  }

  function openDetail(item: TicketItem): void {
    void openContent(projectId, { kind: 'ticket_detail', ticket: item.ticket.ref });
  }

  async function assign(item: TicketItem, to: 'me' | 'none'): Promise<void> {
    const key = item.ticket.ref.key;
    try {
      tickets.patch(await trackerAssign({ ticket: item.ticket.ref, assignee: { kind: to } }));
      toasts.info(to === 'me' ? `${key} assigned to you` : `${key} unassigned`);
    } catch (err) {
      toasts.error(err, `Assigning ${key}`);
    }
  }

  function browse(item: TicketItem): void {
    openExternal({ url: item.ticket.url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  function start(item: TicketItem, preview?: boolean): void {
    void startWorkOnTicket(item.ticket.ref, item.project_ids[0] ?? projectId, { preview });
  }

  /** Opens at once (loading) so the menu, not the pane, gets a digit typed while transitions load. */
  async function openMoveMenu(item: TicketItem): Promise<void> {
    const r = (rowEl(keyOf(item)) ?? root)?.getBoundingClientRect();
    moveMenu = { x: (r?.left ?? 0) + 24, y: (r?.bottom ?? 0) + 2, item, transitions: null };
    const slot = await tickets.loadTransitions(item.ticket.ref);
    if (moveMenu?.item !== item) return;
    if (slot.data) moveMenu.transitions = slot.data;
    else {
      moveMenu = null;
      root?.focus();
      toasts.error(slot.error ?? 'No transitions', `Moving ${item.ticket.ref.key}`);
    }
  }

  function openSourceMenu(): void {
    const r = sourceBtn?.getBoundingClientRect();
    sourceMenu = { x: r?.left ?? 0, y: (r?.bottom ?? 0) + 2 };
  }

  const sourceItems = $derived<MenuItem[]>([
    { id: '', label: 'All sources', icon: viewId === null ? 'check' : undefined },
    ...views.map((v) => ({
      id: v.id,
      label: multiAccount ? `${v.label} (${accountOf(v)})` : v.label,
      icon: v.id === viewId ? 'check' : undefined,
    })),
    { id: '+', label: 'Add source…', icon: 'plus', separator: true },
  ]);

  function shiftLane(delta: number): void {
    if (!cur || mode !== 'board') return;
    const at = columns.findIndex((c) => c.id === columnFor(columns, cur.ticket.status)?.id);
    if (at < 0) return;
    const target = columns[at + delta];
    if (target) void move.moveToColumn(cur.ticket, target, projectIdForColumns);
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
        if (selRow?.kind === 'group') toggleGroup(selRow);
        else if (item) openDetail(item);
        break;
      case '/':
        filterInput?.focus();
        filterInput?.select();
        break;
      case '1':
      case '2':
      case '3':
        setWho(WHOS[Number(e.key) - 1] ?? 'mine');
        break;
      case 'v':
        if (project?.tracker) openSourceMenu();
        break;
      case 'g':
        groupBy = groupBys[(groupBys.indexOf(groupBy) + 1) % groupBys.length] ?? 'status';
        break;
      case 'R':
        refresh();
        break;
      case 'm':
        if (item) void openMoveMenu(item);
        break;
      case 'a':
        if (item) void assign(item, 'me');
        break;
      case 'A':
        if (item) void assign(item, 'none');
        break;
      case 'c':
        if (item) commenting = item;
        break;
      case 'o':
        if (item) browse(item);
        break;
      case 's':
        if (item) start(item);
        break;
      case 'S':
        if (item) start(item, false);
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
    void move.moveToColumn(item.ticket, column, projectIdForColumns);
  }

  const hasWork = (item: TicketItem): boolean => item.work_item_id !== null || workOf(item) !== null;
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="k-listpane"
  data-testid="tickets-pane"
  data-mode={mode}
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Tickets"
  {onkeydown}
>
  <header class="k-toolbar bar">
    <input
      class="k-filter"
      bind:this={filterInput}
      bind:value={filter}
      placeholder="Filter (/)"
      aria-label="Filter tickets"
      onkeydown={filterKeys}
    />
    {#if !noSource}
      <div class="who">
        <Tabs
          label="Whose tickets"
          items={WHOS.map((w) => {
            const n = countOf(w);
            return { id: w, label: n === undefined ? WHO_LABELS[w] : `${WHO_LABELS[w]} ${n}` };
          })}
          value={whoTab ?? undefined}
          onchange={setWho}
        />
      </div>
    {/if}
    {#if scope.kind === 'all'}<span class="title">All projects</span>{/if}
    <span class="spacer"></span>
    {#if project?.tracker}
      <button
        type="button"
        class="source"
        bind:this={sourceBtn}
        aria-haspopup="menu"
        aria-expanded={sourceMenu !== null}
        title="Source (v)"
        onclick={openSourceMenu}
      >
        {viewId ? sourceLabel : 'All sources'}
        <Icon name="chevron-down" size={12} />
      </button>
    {/if}
    {#if mode === 'list' && !noSource}
      <Select
        label="Group (g)"
        value={groupBy}
        options={groupBys.map((g) => ({ value: g, label: GROUP_LABELS[g] }))}
        onchange={(g) => (groupBy = g)}
      />
    {/if}
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

  {#if noSource}
    <EmptyState icon="ticket" title="No ticket source for this project.">
      {#snippet actions()}
        <Button variant="primary" onclick={addSource}>Add source</Button>
      {/snippet}
    </EmptyState>
  {:else if list.loading && !list.data}
    <Loading label="Loading tickets" />
  {:else if !list.data && list.error}
    <ErrorState error={list.error} title="Could not load tickets" onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}
          >{isAuthError(list.error) ? 'Re-authenticate' : 'Open settings'}</Button
        >
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
      {#if whoTab === 'mine'}
        <EmptyState icon="ticket" title={`Nothing assigned to you in ${sourceLabel}.`}>
          {#snippet actions()}<Button onclick={() => setWho('unassigned')}>Show unassigned</Button>{/snippet}
        </EmptyState>
      {:else if whoTab === 'unassigned'}
        <EmptyState icon="ticket" title={`Every ticket in ${sourceLabel} has an owner.`}>
          {#snippet actions()}<Button onclick={() => setWho('anyone')}>Show anyone</Button>{/snippet}
        </EmptyState>
      {:else}
        <EmptyState icon="ticket" title={`No tickets in ${sourceLabel}.`}>
          {#snippet actions()}<Button onclick={refresh}>Refresh</Button>{/snippet}
        </EmptyState>
      {/if}
    {:else if shown.length === 0}
      <EmptyState icon="search" title={`No tickets match "${filter}".`}>
        {#snippet actions()}<Button onclick={() => (filter = '')}>Clear filter</Button>{/snippet}
      </EmptyState>
    {:else if mode === 'list'}
      <div class="list">
        <VirtualList
          bind:this={vlist}
          items={rows}
          itemHeight={ROW_HEIGHT}
          key={(r) => r.key}
          label="Tickets"
          onend={() =>
            void tickets.loadMore(scope, viewId, who).catch((e) => toasts.error(e, 'Loading more tickets'))}
        >
          {#snippet row(r)}
            {#if r.kind === 'group'}
              <button
                type="button"
                tabindex="-1"
                class="k-group head"
                class:selected={selKey === r.key}
                aria-expanded={r.open}
                data-group={r.group.id}
                onclick={() => {
                  selKey = r.key;
                  toggleGroup(r);
                }}
              >
                <Icon name={r.open ? 'chevron-down' : 'chevron-right'} size={12} />
                {r.group.label}
                <span class="count k-num">{r.group.items.length}</span>
              </button>
            {:else}
              {@const item = r.item}
              {@const pr = prOf(item)}
              <button
                type="button"
                tabindex="-1"
                class="k-row"
                class:selected={cur === item}
                aria-current={cur === item ? 'true' : undefined}
                data-key={r.key}
                onclick={() => (selKey = r.key)}
                ondblclick={() => openDetail(item)}
              >
                <span class="k-row-lamp"><Lamp level={sessionsLamp(workOf(item)?.session_ids ?? [])} /></span>
                <span class="k-row-key">{item.ticket.ref.key}</span>
                <span class="k-row-title">{item.ticket.title}</span>
                <span class="meta">
                  {#if pr}
                    <span class="pr" data-pr title={`${pr.title} (CI ${pr.ci})`}
                      ><span class="k-mono">{prLabel(pr)}</span><Lamp
                        level={ciLamp(pr.ci)}
                        title={`CI ${pr.ci}`}
                      /></span
                    >
                  {/if}
                  <span class="status-slot"><StatusChip status={item.ticket.status} /></span>
                  {#if showSource}<Badge>{sourceOf(item)}</Badge>{/if}
                  {#if whoTab !== 'mine'}
                    <span class="k-avatar" title={item.ticket.assignee?.name ?? 'Unassigned'}
                      >{item.ticket.assignee ? initials(item.ticket.assignee.name) : '–'}</span
                    >
                  {/if}
                  <span class="k-row-meta age">{relativeTime(Date.parse(item.ticket.updated_at))}</span>
                </span>
              </button>
            {/if}
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
            <h3>{lane.column?.name ?? 'Other'} <span class="count k-num">{lane.cards.length}</span></h3>
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
                  <span class="card-row first">
                    <span class="key">{item.ticket.ref.key}</span>
                    <Lamp level={sessionsLamp(workOf(item)?.session_ids ?? [])} />
                    {#if hasWork(item)}<Badge tone="accent" title="Local work in progress">work</Badge>{/if}
                  </span>
                  <span class="card-title">{item.ticket.title}</span>
                  <span class="card-row">
                    <span class="k-avatar" title={item.ticket.assignee?.name ?? 'Unassigned'}
                      >{item.ticket.assignee ? initials(item.ticket.assignee.name) : '–'}</span
                    >
                    {#each item.ticket.labels.slice(0, 2) as l (l)}<Badge>{l}</Badge>{/each}
                    <span class="spacer"></span>
                    {#if item.ticket.priority}<span class="k-row-meta">{item.ticket.priority}</span>{/if}
                    <span class="k-row-meta">{relativeTime(Date.parse(item.ticket.updated_at))}</span>
                  </span>
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
              void tickets.loadMore(scope, viewId, who).catch((e) => toasts.error(e, 'Loading more tickets'))}
          >
            <Icon name="plus" size={12} /> More
          </Button>
        {/if}
      </div>
    {/if}
  {/if}

  <KeyHints
    hints={[
      ['j/k', 'move'],
      ['Enter', 'open'],
      ['1/2/3', 'who'],
      ['m', 'move to'],
      ['a/A', 'assign me, unassign'],
      ['s/S', 'start, start now'],
      ['v', 'source'],
      ['g', 'group'],
      ['c', 'comment'],
      ['o', 'browser'],
    ]}
  />
</div>

{#if moveMenu}
  <MoveMenu
    ticket={moveMenu.item.ticket}
    transitions={moveMenu.transitions}
    x={moveMenu.x}
    y={moveMenu.y}
    onselect={(t) => moveMenu && void move.moveViaTransition(moveMenu.item.ticket, t)}
    onclose={() => {
      moveMenu = null;
      root?.focus();
    }}
  />
{/if}
{#if sourceMenu}
  <Menu
    items={sourceItems}
    x={sourceMenu.x}
    y={sourceMenu.y}
    label="Source"
    onselect={(id) => (id === '+' ? addSource() : setView(id === '' ? null : id))}
    onclose={() => {
      sourceMenu = null;
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
  /* One line at any pane width: the toolbar scrolls sideways instead of wrapping labels. */
  .bar {
    overflow-x: auto;
    scrollbar-width: none;
    white-space: nowrap;
  }

  .bar .k-filter {
    flex: 0 1 180px;
    min-width: 72px;
  }

  .who {
    font-variant-numeric: tabular-nums;
  }

  /* Fixed width so the category bars form one column down the list. */
  .status-slot {
    display: inline-flex;
    width: 112px;
  }

  .status-slot :global(span) {
    max-width: 100%;
  }

  .source {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    height: 22px;
    padding: 0 var(--k-space-3);
    border: 0;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
  }

  .source:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .head {
    width: 100%;
    border: 0;
    background: transparent;
    text-align: left;
    cursor: default;
  }

  .head.selected {
    background: var(--k-bg-selected);
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  .meta {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-4);
  }

  .pr {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .age {
    min-width: 52px;
    text-align: right;
  }

  /* Lanes are bezel trays on the well, so each lane (and an empty drop target) has an edge. */
  .board {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: var(--k-space-3);
    padding: var(--k-space-3);
    background: var(--k-well);
    overflow: auto;
  }

  .lane {
    flex: 0 0 264px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-radius: var(--k-radius-sm);
    background: var(--k-bezel);
  }

  .lane.over {
    box-shadow: inset 0 2px 0 var(--k-accent);
  }

  .lane h3 {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    margin: 0;
    padding: var(--k-space-3) var(--k-space-3) var(--k-space-2);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg-muted);
  }

  .cards {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: 0 var(--k-space-2) var(--k-space-2);
    overflow: auto;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: var(--k-well);
    color: inherit;
    text-align: left;
    cursor: grab;
  }

  .card.selected {
    background: var(--k-bg-selected);
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  .card-row {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  /* Fixed to chip height on every card so titles line up across lanes. */
  .card-row.first {
    height: 18px;
  }

  .key {
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .card-title {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
</style>
