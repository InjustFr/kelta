<script lang="ts">
  import { tick, untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { Column, PrLink, Ticket, TicketItem, TicketSort, TicketsMode, Who } from '$lib/gen';
  import { openExternal, trackerAssign } from '$lib/ipc/commands';
  import { findPane, replacePaneContent } from '$lib/layout';
  import { layout, projects, settings, tickets, toasts, ui, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    Icon,
    Lamp,
    Menu,
    Select,
    Tabs,
    ROW_HEIGHT,
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
  import StatusChip from './StatusChip.svelte';
  import StatusPicker from './StatusPicker.svelte';
  import TicketDetail from './TicketDetail.svelte';
  import {
    GROUP_BYS,
    SORTS,
    ageDays,
    ageLevel,
    flowOf,
    groupTickets,
    sortTickets,
    type GroupBy,
    type TicketGroup,
  } from './group';
  import { MoveController } from './move.svelte';
  import PrChip from './PrChip.svelte';
  import { ensureReviews, mainPr, openPr, openPrs, prLabel, prMenuItems } from './prs';
  import RowButton from './RowButton.svelte';

  let { projectId, paneId, content, focused }: PaneProps<'tickets'> = $props();

  const WHOS: readonly Who[] = ['mine', 'unassigned', 'anyone'];
  const WHO_LABELS: Record<Who, string> = { mine: 'Mine', unassigned: 'Unassigned', anyone: 'Anyone' };
  const GROUP_LABELS: Record<GroupBy, string> = {
    flow: 'By flow',
    status: 'By status',
    priority: 'By priority',
    sprint: 'By sprint',
    assignee: 'By assignee',
    source: 'By source',
    none: 'No grouping',
  };
  const SORT_LABELS: Record<TicketSort, string> = {
    priority: 'Sort by priority',
    updated: 'Sort by updated',
    age: 'Sort by age in status',
    key: 'Sort by key',
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
  const viewId = $derived.by(() => {
    const v = viewOverride !== undefined ? viewOverride : content.view_id;
    // A removed or renamed source falls back to all sources instead of another one.
    return v && project?.tracker && !views.some((x) => x.id === v) ? null : v;
  });
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
  let groupOverride = $state<GroupBy | undefined>(undefined);
  let sortOverride = $state<TicketSort | undefined>(undefined);
  const sort = $derived<TicketSort>(sortOverride ?? content.sort ?? 'priority');
  let personOverride = $state<string | null | undefined>(undefined);
  /** Assignee user id the list narrows to (on top of Anyone); client-side. */
  const person = $derived(personOverride !== undefined ? personOverride : (content.person ?? null));
  /** "Current sprint" quick filter (`f s`), client-side on `sprint.active`. */
  let currentSprint = $state(false);

  function persist(patch: Partial<Omit<typeof content, 'kind' | 'scope'>>): void {
    const current = layout.get(projectId);
    if (!current || !current.tabs.some((t) => findPane(t.root, paneId))) return;
    layout.update(projectId, (l) => ({
      ...l,
      // Merged onto the stored content: two changes in one tick must not drop the first.
      tabs: l.tabs.map((t) => {
        const stored = findPane(t.root, paneId)?.pane.content;
        const base = stored?.kind === 'tickets' ? stored : content;
        return { ...t, root: replacePaneContent(t.root, paneId, { ...base, ...patch }) };
      }),
    }));
  }

  function setView(id: string | null): void {
    viewOverride = id;
    persist({ view_id: id });
  }

  function setWho(w: Who): void {
    whoOverride = w;
    personOverride = null;
    persist({ who: w, person: null });
  }

  /** A person loads Anyone and keeps that assignee's tickets; `null` = everyone in the who. */
  function setPerson(id: string | null): void {
    personOverride = id;
    if (id) whoOverride = 'anyone';
    persist(id ? { person: id, who: 'anyone' } : { person: null });
  }

  function setGroup(g: GroupBy): void {
    groupOverride = g;
    persist({ group: g });
  }

  function setSort(s: TicketSort): void {
    sortOverride = s;
    persist({ sort: s });
  }

  function setMode(m: TicketsMode): void {
    modeOverride = m;
    persist({ mode: m });
  }

  function addSource(): void {
    ui.openSheet('tracker.source_picker', { projectId: project?.id ?? projectId });
  }

  // ---- data ---------------------------------------------------------------------------------
  const list = $derived(tickets.list(scope, viewId, who));
  const items = $derived(list.data?.items ?? []);
  let filter = $state('');
  const shown = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    return items.filter((i) => {
      const t = i.ticket;
      if (person && t.assignee?.id !== person) return false;
      if (currentSprint && !t.sprint?.active) return false;
      return (
        needle === '' ||
        `${t.ref.key} ${t.title} ${t.status.name} ${t.kind ?? ''} ${t.labels.join(' ')} ${t.assignee?.name ?? ''}`
          .toLowerCase()
          .includes(needle)
      );
    });
  });

  // shortcut: people come from the loaded first page per source, upgrade = a tracker user search.
  const people = $derived.by(() => {
    const all: Record<string, string> = person ? { [person]: person } : {};
    for (const i of tickets.list(scope, viewId, 'anyone').data?.items ?? items)
      if (i.ticket.assignee) all[i.ticket.assignee.id] = i.ticket.assignee.name;
    return Object.entries(all).sort((a, b) => a[1].localeCompare(b[1]));
  });
  const hasSprints = $derived(items.some((i) => i.ticket.sprint));

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
      if (settings.value() === null) void settings.load().catch(() => undefined);
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
  const groupBy = $derived.by<GroupBy>(() => {
    const g = groupOverride ?? content.group ?? 'flow';
    return groupBys.includes(g) ? g : 'flow';
  });
  const wipLimit = $derived(settings.value(project?.id)?.tickets?.wip_limit ?? 3);
  const sorted = $derived(sortTickets(shown, sort));
  const groups = $derived(
    groupTickets(sorted, groupBy, views, (item) => {
      const w = workOf(item);
      return flowOf(
        item,
        { work: w, needsYou: sessionsLamp(w?.session_ids ?? []) === 'needs_input' },
        Date.now(),
      );
    }),
  );
  const rows = $derived.by<Row[]>(() => {
    if (groupBy === 'none') return sorted.map((item) => ({ kind: 'ticket', key: keyOf(item), item }));
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

  const workOf = (item: TicketItem) =>
    work.forTicket(item.ticket.ref) ?? (item.work_item_id ? work.get(item.work_item_id) : null);
  const prOf = (item: TicketItem) => mainPr(item.prs);

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
  /** The status picker: one ticket, or a selection (moves shared by target status name). */
  let picker = $state.raw<{ tickets: Ticket[]; anchor: HTMLElement | null } | null>(null);
  let prMenu = $state<{ x: number; y: number; prs: PrLink[]; projectId: string; browser: boolean } | null>(
    null,
  );
  /** Multi-selection (`x`, Shift+j/k), as row keys. */
  let picked = $state<string[]>([]);
  const pickedItems = $derived(shown.filter((i) => picked.includes(keyOf(i))));
  /** First key of a two-key chord (`f s`). */
  let chord: string | null = null;
  let sourceMenu = $state<{ x: number; y: number } | null>(null);
  let commenting = $state<TicketItem | null>(null);

  // ---- split view (T3): the selected ticket's detail in a column on the right ---------------
  // shortcut: on/off is per mount, not persisted; add `PaneContent::Tickets.split` if it should stick.
  let split = $state(true);
  let paneWidth = $state(0);
  let side = $state<HTMLElement>();
  /** Below ~720px the list would be too narrow: the detail opens as its own pane instead. */
  const wide = $derived(paneWidth >= 720);
  const splitShown = $derived(split && wide && mode === 'list');

  /** Enter: the detail column takes the keyboard (narrow pane: the standalone pane). */
  async function focusDetail(item: TicketItem): Promise<void> {
    if (!wide || mode !== 'list') return openDetail(item);
    split = true;
    await tick();
    side?.querySelector<HTMLElement>('[data-testid="ticket-detail"]')?.focus();
  }

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

  /** Opens the status picker under the first ticket's status chip (its card on the board). */
  function openPicker(list: TicketItem[]): void {
    const [item] = list;
    if (!item) return;
    const el = rowEl(keyOf(item));
    picker = {
      tickets: list.map((i) => i.ticket),
      anchor: el?.querySelector<HTMLElement>('[data-status]') ?? el ?? null,
    };
  }

  function closePicker(): void {
    const was = picker;
    picker = null;
    root?.focus();
    // A selection that moved is done with; one dismissed (Esc) stays for another try.
    const now = (t: Ticket) => items.find((i) => keyOf(i) === ticketKey(t.ref))?.ticket.status.name;
    if (was && was.tickets.length > 1 && was.tickets.some((t) => now(t) !== t.status.name)) picked = [];
  }

  /** `p` / `P`: the only PR at once, else a picker. */
  function openPrOf(item: TicketItem, browser: boolean): void {
    const pid = item.project_ids[0] ?? projectId;
    if (!openPrs(item.ticket.ref.key, item.prs, pid, browser)) return;
    const r = (rowEl(keyOf(item)) ?? root)?.getBoundingClientRect();
    prMenu = { x: (r?.left ?? 0) + 24, y: (r?.bottom ?? 0) + 2, prs: item.prs, projectId: pid, browser };
  }

  /** The tracker can do it, else a toast that says so and the way out. */
  function can(item: TicketItem, cap: 'assign' | 'comment'): boolean {
    if (item.caps[cap]) return true;
    const what = cap === 'assign' ? 'change assignees' : 'take comments';
    toasts.info(
      `${item.ticket.ref.account} can't ${what} from Kelta. Open ${item.ticket.ref.key} in the browser (o).`,
    );
    return false;
  }

  function togglePick(key: string): void {
    picked = picked.includes(key) ? picked.filter((k) => k !== key) : [...picked, key];
  }

  /** Shift+j/k: adds the current row and the next one to the selection. */
  function extend(delta: number): void {
    const from = selKey;
    step(delta);
    const add = [from, selKey].filter((k): k is string => k !== null && !k.startsWith('group:'));
    picked = [...new Set([...picked, ...add])];
  }

  function pickOrMove(item: TicketItem): void {
    openPicker(picked.length > 0 && picked.includes(keyOf(item)) ? pickedItems : [item]);
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
    // The detail column runs its own keys; Esc hands the keyboard back to the list.
    if (side?.contains(target)) {
      if (e.key === 'Escape') {
        root?.focus();
        e.preventDefault();
      }
      return;
    }
    const item = cur;
    const first = chord;
    chord = null;
    if (first === 'f' && e.key === 's') {
      currentSprint = !currentSprint;
      e.preventDefault();
      return;
    }
    switch (e.key) {
      case 'j':
      case 'ArrowDown':
        if (e.shiftKey) extend(1);
        else step(1);
        break;
      case 'k':
      case 'ArrowUp':
        if (e.shiftKey) extend(-1);
        else step(-1);
        break;
      case 'J':
        extend(1);
        break;
      case 'K':
        extend(-1);
        break;
      case 'x':
        if (item && mode === 'list') togglePick(keyOf(item));
        break;
      case 'Escape':
        if (picked.length === 0) return;
        picked = [];
        break;
      case 'f':
        chord = 'f';
        break;
      case 'p':
      case 'P':
        if (item) openPrOf(item, e.key === 'P');
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
        if (e.shiftKey) {
          if (item) openDetail(item);
        } else if (selRow?.kind === 'group') toggleGroup(selRow);
        else if (item) void focusDetail(item);
        break;
      case ' ':
        if (mode !== 'list') return;
        if (wide) split = !split;
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
        setGroup(groupBys[(groupBys.indexOf(groupBy) + 1) % groupBys.length] ?? 'flow');
        break;
      case 'R':
        refresh();
        break;
      case 'm':
        if (picked.length > 0) openPicker(pickedItems);
        else if (item) openPicker([item]);
        break;
      case 'a':
        if (item && can(item, 'assign')) void assign(item, 'me');
        break;
      case 'A':
        if (item && can(item, 'assign')) void assign(item, 'none');
        break;
      case 'c':
        if (item && can(item, 'comment')) commenting = item;
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
  bind:clientWidth={paneWidth}
  tabindex="0"
  role="group"
  aria-label="Tickets"
  {onkeydown}
>
  <header class="k-toolbar bar">
    {#if scope.kind === 'all'}<span class="title">All projects</span>{/if}
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
        {#if people.length > 0 || person}
          <Select
            label="Person"
            value={person ?? ''}
            options={[
              { value: '', label: 'Any person' },
              ...people.map(([value, label]) => ({ value, label })),
            ]}
            onchange={(id) => setPerson(id === '' ? null : id)}
          />
        {/if}
      </div>
    {/if}
    {#if hasSprints || currentSprint}
      <Button
        variant="ghost"
        size="sm"
        icon={currentSprint ? 'check' : undefined}
        aria-pressed={currentSprint}
        title="Only tickets in an active sprint (f, then s)"
        onclick={() => (currentSprint = !currentSprint)}>Current sprint</Button
      >
    {/if}
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
        onchange={setGroup}
      />
      <Select
        label="Sort"
        value={sort}
        options={SORTS.map((x) => ({ value: x, label: SORT_LABELS[x] }))}
        onchange={setSort}
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
    <Button variant="ghost" size="sm" icon="refresh-cw" chord="shift+r" onclick={refresh}>Refresh</Button>
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
          >{isAuthError(list.error) ? 'Re-authenticate' : 'Open account settings'}</Button
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
      {#if list.data?.errors.length}
        <!-- the banner above says what failed; "no tickets" would be a false claim -->
      {:else if whoTab === 'mine'}
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
    {:else if mode === 'list' && groupBy === 'flow' && rows.length === 0}
      <EmptyState icon="ticket" title="Nothing in flow; older done tickets are hidden.">
        {#snippet actions()}<Button onclick={() => setGroup('status')}>By status</Button>{/snippet}
      </EmptyState>
    {:else if mode === 'list'}
      <div class="split">
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
                {@const overWip =
                  groupBy === 'flow' &&
                  r.group.id === 'doing' &&
                  r.group.items.filter(hasWork).length > wipLimit}
                <button
                  type="button"
                  tabindex="-1"
                  class="k-group head"
                  class:selected={selKey === r.key}
                  class:wip={overWip}
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
                  {#if overWip}<span class="wip-note">Above your limit of {wipLimit}</span>{/if}
                </button>
              {:else}
                {@const item = r.item}
                {@const t = item.ticket}
                {@const pr = prOf(item)}
                {@const isPicked = picked.includes(r.key)}
                {@const age = ageLevel(t, Date.now())}
                <button
                  type="button"
                  tabindex="-1"
                  class="k-row"
                  class:selected={cur === item}
                  class:picked={isPicked}
                  aria-current={cur === item ? 'true' : undefined}
                  aria-pressed={picked.length > 0 ? isPicked : undefined}
                  data-key={r.key}
                  onclick={(e) => {
                    selKey = r.key;
                    if (e.metaKey || e.ctrlKey) togglePick(r.key);
                  }}
                  ondblclick={() => openDetail(item)}
                >
                  <span class="k-row-lamp"
                    >{#if isPicked}<Icon name="check" size={12} />{:else}<Lamp
                        level={sessionsLamp(workOf(item)?.session_ids ?? [])}
                      />{/if}</span
                  >
                  <span class="k-row-key">{t.ref.key}</span>
                  <span class="k-row-title"
                    >{t.title}<span class="acts" data-acts>
                      <RowButton
                        icon="arrow-right"
                        label="Move (m)"
                        onclick={() => {
                          selKey = r.key;
                          pickOrMove(item);
                        }}
                      />
                      <RowButton
                        icon="git-pull-request"
                        label={item.prs.length > 0 ? 'Open pull request (p)' : 'No pull request linked'}
                        disabled={item.prs.length === 0}
                        onclick={() => openPrOf(item, false)}
                      />
                      <RowButton
                        icon="play"
                        label={hasWork(item) ? 'Resume work (s)' : 'Start work (s)'}
                        onclick={() => start(item)}
                      /></span
                    ></span
                  >
                  <span class="meta">
                    {#if pr}<span class="pr" data-pr
                        ><RowButton label={`Open ${prLabel(pr)} (p)`} onclick={() => openPrOf(item, false)}
                          ><PrChip {pr} /></RowButton
                        ></span
                      >{/if}
                    {#if t.sprint && groupBy !== 'sprint'}
                      <span
                        class="sprint"
                        class:active={t.sprint.active}
                        title={t.sprint.active ? `${t.sprint.name} (current)` : t.sprint.name}
                        >{t.sprint.name}</span
                      >
                    {/if}
                    <span class="status-slot" data-status
                      ><RowButton
                        label={`${t.status.name}: move ${t.ref.key} (m)`}
                        onclick={() => {
                          selKey = r.key;
                          pickOrMove(item);
                        }}><StatusChip status={t.status} /></RowButton
                      ></span
                    >
                    <span class="age-slot"
                      >{#if age}<span
                          class="aged {age}"
                          data-age={age}
                          title={`${ageDays(t, Date.now())} days in ${t.status.name}`}
                          >{ageDays(t, Date.now())}d</span
                        >{/if}</span
                    >
                    {#if showSource}<span class="source-slot"><Badge>{sourceOf(item)}</Badge></span>{/if}
                    {#if whoTab !== 'mine'}
                      <span class="k-avatar" title={item.ticket.assignee?.name ?? 'Unassigned'}
                        >{item.ticket.assignee ? initials(item.ticket.assignee.name) : '–'}</span
                      >
                    {/if}
                    <!-- An aged row shows one duration (days in status); the empty slot keeps the column. -->
                    <span class="k-row-meta age"
                      >{age ? '' : relativeTime(Date.parse(item.ticket.updated_at))}</span
                    >
                  </span>
                </button>
              {/if}
            {/snippet}
          </VirtualList>
        </div>
        {#if splitShown}
          <aside class="side" bind:this={side} aria-label="Ticket detail">
            {#if cur}
              <TicketDetail item={cur} projectId={cur.project_ids[0] ?? projectId} embedded />
            {:else}
              <p class="side-empty">Select a ticket to see it here.</p>
            {/if}
          </aside>
        {/if}
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
      ['j k', 'Move'],
      ['space', 'Split'],
      ['enter', 'Open'],
      ['shift+enter', 'Own pane'],
      ['1 2 3', 'Who'],
      ['/', 'Filter'],
      ['m', 'Move to'],
      ['x', 'Select'],
      ['p shift+p', 'Pull request, in browser'],
      ['f then s', 'Current sprint'],
      ['a shift+a', 'Assign me, unassign'],
      ['s shift+s', 'Start work, start now'],
      ['v', 'Source'],
      ['g', 'Group'],
      ['c', 'Comment'],
      ['o', 'Open in browser'],
      ['shift+r', 'Refresh'],
    ]}
  />
</div>

{#if picker}
  <StatusPicker tickets={picker.tickets} {projectId} anchor={picker.anchor} onclose={closePicker} />
{/if}
{#if prMenu}
  {@const m = prMenu}
  <Menu
    items={prMenuItems(m.prs)}
    x={m.x}
    y={m.y}
    label="Open pull request"
    onselect={(url) => {
      const pr = m.prs.find((p) => p.url === url);
      if (pr) openPr(pr, m.projectId, m.browser);
    }}
    onclose={() => {
      prMenu = null;
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
  /* A narrow pane wraps the toolbar to a second line rather than hiding controls; labels never wrap. */
  .bar {
    flex-wrap: wrap;
    row-gap: var(--k-space-2);
    height: auto;
    min-height: var(--k-tabbar-height);
    padding-block: var(--k-space-2);
    white-space: nowrap;
  }

  .bar .k-filter {
    flex: 0 1 180px;
    min-width: 72px;
  }

  .who {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    font-variant-numeric: tabular-nums;
  }

  /* Fixed width so the category bars form one column down the list. */
  .status-slot {
    display: inline-flex;
    flex: none;
    width: 112px;
  }

  .status-slot :global(span) {
    max-width: 100%;
  }

  /* Fixed too, so source names of any length do not shift the status column. */
  .source-slot {
    display: inline-flex;
    flex: none;
    justify-content: flex-end;
    width: 88px;
  }

  .source-slot :global(.k-badge) {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Split view: list left, the selected ticket's detail right, the 1px splitter gap between. */
  .split {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 1px;
    background: var(--k-border);
  }

  .split .list {
    min-width: 0;
    background: var(--k-well);
  }

  .side {
    flex: 0 0 clamp(320px, 44%, 640px);
    min-width: 0;
    background: var(--k-well);
  }

  .side-empty {
    margin: 0;
    padding: var(--k-space-5) var(--k-space-4);
    color: var(--k-fg-subtle);
  }

  /* The title is the row: the meta shrinks first, then drops the source and avatar at split widths. */
  .list {
    container-type: inline-size;
  }

  .k-row-title {
    position: relative;
    min-width: 120px;
  }

  @container (width < 560px) {
    .source-slot,
    .sprint,
    .meta .k-avatar {
      display: none;
    }
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
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-4);
  }

  .pr {
    display: inline-flex;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .age {
    min-width: 52px;
    text-align: right;
  }

  /* Status age: quiet at 7 days, --k-warn at 14, --k-danger at 21; text tint only (DESIGN §6.11). */
  .age-slot {
    display: inline-flex;
    flex: none;
    justify-content: flex-end;
    width: 28px;
  }

  .aged {
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-muted);
  }

  .aged.warn {
    color: var(--k-warn);
  }

  .aged.danger {
    color: var(--k-danger);
  }

  .sprint {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .sprint.active {
    color: var(--k-fg-muted);
  }

  /* Compact row actions: on the selected row, on hover and on focus-within (DESIGN §7), laid over
     the end of the title on the row's own backing so showing them never reflows the row. */
  .acts {
    position: absolute;
    inset: 0 0 0 auto;
    display: none;
    align-items: center;
    gap: var(--k-space-1);
    padding-left: var(--k-space-2);
    background: linear-gradient(var(--k-bg-hover), var(--k-bg-hover)), var(--k-well);
  }

  .k-row.selected .acts,
  .k-row.picked .acts {
    background: var(--k-bg-selected);
  }

  .k-row:hover .acts,
  .k-row.selected .acts,
  .k-row:focus-within .acts {
    display: inline-flex;
  }

  .k-row.picked {
    background: var(--k-bg-selected);
  }

  .k-row.picked .k-row-lamp {
    color: var(--k-accent);
  }

  .head.wip {
    color: var(--k-warn);
  }

  .wip-note {
    font-weight: 400;
    font-size: var(--k-font-size-xs);
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
    height: 20px;
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
