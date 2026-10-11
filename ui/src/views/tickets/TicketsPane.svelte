<script lang="ts">
  import { tick, untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { Column, PrLink, Ticket, TicketItem, TicketSort, TicketsMode, Who } from '$lib/gen';
  import { openExternal, trackerAssign } from '$lib/ipc/commands';
  import { GLOBAL, projects, settings, tickets, toasts, ui, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import {
    Badge,
    Button,
    Dialog,
    EmptyState,
    ErrorState,
    Icon,
    Kbd,
    Lamp,
    Menu,
    ROW_HEIGHT,
    VirtualList,
    relativeTime,
    type MenuItem,
  } from '$lib/ui';

  import { columnFor, initials, isAuthError } from '../work/common';
  import { openFrom, persistPaneContent } from '../work/nav';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { startWorkOnTicket } from '../work/startWork';
  import { batch, batchKey } from '../work/batch.svelte';
  import { runPrimary } from '../work/actions';
  import { phaseNow } from '../work/live';
  import CommentDialog from './CommentDialog.svelte';
  import MoveDialogs from './MoveDialogs.svelte';
  import StatusPicker from './StatusPicker.svelte';
  import TicketDetail from './TicketDetail.svelte';
  import { GROUP_BYS, flowOf, groupTickets, sortTickets, type GroupBy, type TicketGroup } from './group';
  import { MoveController } from './move.svelte';
  import { ensureReviews, openPr, openPrs, prMenuItems } from './prs';
  import TicketRow from './TicketRow.svelte';
  import TicketsToolbar from './TicketsToolbar.svelte';
  import { pickWorkflow } from './workflow.svelte';

  let { projectId, paneId, content, focused }: PaneProps<'tickets'> = $props();
  /** The pane's project; `null` in the rail's Tickets view, where each ticket uses its own project. */
  const home = $derived(projectId === GLOBAL ? null : projectId);

  const WHOS: readonly Who[] = ['mine', 'unassigned', 'anyone'];
  const WHO_LABELS: Record<Who, string> = { mine: 'Mine', unassigned: 'Unassigned', anyone: 'Team' };
  const move = new MoveController();

  // ---- scope, source, who and mode ----------------------------------------------------------
  const scope = $derived(content.scope);
  const project = $derived(scope.kind === 'project' ? projects.byId(scope.id) : null);
  /** The `[[sources]]` of the scope: all of them, or those feeding the project. */
  const sources = $derived(
    (settings.value()?.sources ?? []).filter(
      (s) =>
        s.tracker.views.length > 0 &&
        (scope.kind === 'all' ||
          (scope.kind === 'project' ? s.projects.includes(scope.id) : s.id === scope.id)),
    ),
  );
  let sourceOverride = $state<string | null | undefined>(undefined);
  let viewOverride = $state<string | null | undefined>(undefined);
  let whoOverride = $state<Who | null | undefined>(undefined);
  /** The pane's picks as saved; the core resolves them (`TicketPage.scope` / `view_id`). */
  const sourcePick = $derived(sourceOverride !== undefined ? sourceOverride : (content.source_id ?? null));
  const viewPick = $derived(viewOverride !== undefined ? viewOverride : (content.view_id ?? null));
  /** The persisted who the list loads with (`null` = each view decides). */
  const who = $derived(whoOverride !== undefined ? whoOverride : content.who);
  const list = $derived(tickets.list(scope, viewPick, who, sourcePick));
  /** The source the list narrows to, as the core resolved it (a removed one, or one no longer
   * feeding the project, falls back to all sources). */
  const source = $derived.by(() => {
    const s = list.data?.scope;
    return s?.kind === 'source' ? (sources.find((x) => x.id === s.id) ?? null) : null;
  });
  /** `null` = all views of the source (or of every source), as the core resolved it. */
  const viewId = $derived(list.data?.view_id ?? null);
  /** The one source in effect (board mode and its columns need one). */
  const single = $derived(source ?? (sources.length === 1 ? sources[0]! : null));
  const views = $derived(sources.flatMap((s) => s.tracker.views));
  const sourceLabel = $derived(
    source?.label ?? (project ? `${project.name}'s ticket sources` : 'your ticket sources'),
  );
  /** The source button and `v` only when the menu offers more than one choice. */
  const menuChoices = $derived(sources.length > 1 || (sources[0]?.tracker.views.length ?? 0) > 1);
  const noSource = $derived(settings.hasSources() !== null && sources.length === 0);

  /** The tab shown: the pane's who, else the who every queried view agrees on. */
  const whoTab = $derived.by<Who | null>(() => {
    if (who) return who;
    const shown = source ? source.tracker.views.filter((v) => !viewId || v.id === viewId) : views;
    const ws = new Set(shown.map((v) => v.who));
    return ws.size === 1 ? ([...ws][0] ?? null) : null;
  });

  let modeOverride = $state<TicketsMode | undefined>(undefined);
  const wantedMode = $derived(modeOverride ?? content.mode);
  const mode = $derived<TicketsMode>(wantedMode === 'board' && single ? 'board' : 'list');
  let groupOverride = $state<GroupBy | undefined>(undefined);
  let sortOverride = $state<TicketSort | undefined>(undefined);
  const sort = $derived<TicketSort>(sortOverride ?? content.sort ?? 'priority');
  let personOverride = $state<string | null | undefined>(undefined);
  /** Person key (or a tracker user id saved before team people) the list narrows to; client-side. */
  const person = $derived(personOverride !== undefined ? personOverride : (content.person ?? null));
  /** "Current sprint" quick filter (`f s`), client-side on `sprint.active`. */
  let currentSprint = $state(false);

  function persist(patch: Partial<Omit<typeof content, 'kind' | 'scope'>>): void {
    persistPaneContent(projectId, paneId, patch);
  }

  function setSource(id: string | null, view: string | null = null): void {
    sourceOverride = id;
    viewOverride = view;
    persist({ source_id: id, view_id: view });
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

  /** The source picker in source mode: a new source, feeding this project when the pane has one. */
  function addSource(): void {
    ui.openSheet('tracker.source_picker', {
      projectId: project && !project.builtin ? project.id : undefined,
    });
  }

  // ---- data ---------------------------------------------------------------------------------
  const items = $derived(list.data?.items ?? []);
  let filter = $state('');
  const shown = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    return items.filter((i) => {
      const t = i.ticket;
      if (person && !isPerson(t)) return false;
      if (currentSprint && !t.sprint?.active) return false;
      return (
        needle === '' ||
        `${t.ref.key} ${t.title} ${t.status.name} ${t.kind ?? ''} ${t.labels.join(' ')} ${t.assignee?.name ?? ''}`
          .toLowerCase()
          .includes(needle)
      );
    });
  });

  // Team people with a tracker identity (the core merges them across trackers and code hosts): any
  // tab's page names the scope's cached Team list; the Anyone page, once loaded, is the freshest.
  const people = $derived(
    ((tickets.list(scope, viewPick, 'anyone', sourcePick).data ?? list.data)?.people ?? [])
      .filter((p) => p.ids.some((i) => !i.code_host))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );
  /** The picked person's entry, by key or by a user id saved before team people. */
  const personEntry = $derived(
    person
      ? people.find(
          (p) => p.key === person || p.ids.some((i) => i.id === person || `${i.account}:${i.id}` === person),
        )
      : undefined,
  );
  const isPerson = (t: Ticket): boolean =>
    personEntry
      ? personEntry.ids.some((i) => !i.code_host && i.account === t.ref.account && i.id === t.assignee?.id)
      : t.assignee?.id === person;
  const hasSprints = $derived(items.some((i) => i.ticket.sprint));

  $effect(() => {
    const [s, v, w, src] = [scope, viewPick, who, sourcePick];
    if (noSource) return;
    untrack(() => {
      void tickets.load(s, v, false, w, src);
      // Counts for the other tabs (served from the core's cache); Anyone can be large, so only on demand.
      for (const o of ['mine', 'unassigned'] as const) {
        const l = tickets.list(s, v, o, src);
        if (o !== whoTab && !l.data && !l.loading) void tickets.load(s, v, false, o, src);
      }
      ensureReviews();
      if (settings.value() === null) void settings.load().catch(() => undefined);
    });
  });

  const countOf = (w: Who): number | undefined =>
    (w === whoTab ? list : tickets.list(scope, viewPick, w, sourcePick)).data?.items.length;

  const columnsSource = $derived(single?.id ?? null);
  $effect(() => {
    const id = columnsSource;
    if (id && (mode === 'board' || groupBy === 'status')) untrack(() => void tickets.loadColumns(id));
  });

  const columnsState = $derived(columnsSource ? tickets.columns[columnsSource] : undefined);
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
  const groupBy = $derived.by<GroupBy>(() => {
    const g = groupOverride ?? content.group ?? 'flow';
    return GROUP_BYS.includes(g) ? g : 'flow';
  });
  const wipLimit = $derived(settings.value(project?.id)?.tickets?.wip_limit ?? 3);
  const sorted = $derived(sortTickets(shown, sort));
  const groups = $derived(
    groupTickets(
      sorted,
      groupBy,
      sources,
      (item) => {
        const w = workOf(item);
        // Durable Claude state (FLOW §2.3): a reply waiting on me counts, not only a live prompt.
        return flowOf(item, { work: w, needsYou: phaseFor(item)?.section === 'needs_you' }, Date.now());
      },
      columns,
    ),
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

  /** The key column fits the longest key in view, capped so a long key cannot take the title's room. */
  const keyChars = $derived(Math.min(12, Math.max(4, ...shown.map((i) => i.ticket.ref.key.length))));
  const showSource = $derived(groupBy !== 'source' && !source && sources.length > 1);
  const sourceOf = (item: TicketItem): string =>
    sources.find((s) => s.id === item.source_ids[0])?.label ?? 'Other';

  const workOf = (item: TicketItem) =>
    work.forTicket(item.ticket.ref) ?? (item.work_item_id ? work.get(item.work_item_id) : null);
  const liveWork = (item: TicketItem) => {
    const w = workOf(item);
    return w?.state.kind === 'finished' ? null : w;
  };
  /** The work item's phase (FLOW §2.2), as Now shows it. */
  const phaseFor = (item: TicketItem) => {
    const w = liveWork(item);
    return w ? phaseNow(w) : null;
  };

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
  /** What Enter will do on the selected row, named before it is pressed (mirrors the Enter key below). */
  const enterLabel = $derived.by(() => {
    const ph = cur && mode === 'list' && !splitShown ? phaseFor(cur) : null;
    return ph ? (ph.primary ? ph.primaryLabel : 'Go to work tab') : 'Open';
  });

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

  // A PR's linked ticket from Reviews (`ui.select`) lands on that ticket here.
  $effect(() => {
    const want = ui.select;
    if (want?.view !== 'tickets' || projectId !== GLOBAL) return;
    selKey = want.key;
    ui.select = null;
  });

  $effect(() => {
    if (!focused || !cur) return;
    selectTicket(cur.ticket.ref, cur.project_ids[0] ?? null);
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
  /** The full key list (`?`); the footer shows the first five. */
  let keysOpen = $state(false);
  const hints = $derived<[string, string][]>([
    ['j k', 'Move'],
    ['enter', enterLabel],
    ['s shift+s', 'Start work, start now'],
    ['w', 'Run workflow'],
    ['m', 'Move to'],
    ['/', 'Filter'],
    ['space', 'Split'],
    ['g', 'Open detail'],
    ['shift+enter', 'Own pane'],
    ['1 2 3', 'Who'],
    ['mod+enter', 'Start marked'],
    ['x', 'Select'],
    ['p shift+p', 'Pull request, in browser'],
    ['f then s', 'Current sprint'],
    ['a shift+a', 'Assign me, unassign'],
    ['v', 'Source'],
    ['shift+g', 'Group'],
    ['c', 'Comment'],
    ['o', 'Open in browser'],
    ['shift+r', 'Refresh'],
  ]);

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
    void tickets.load(scope, viewPick, true, who, sourcePick);
    if (columnsSource && (mode === 'board' || groupBy === 'status')) void tickets.loadColumns(columnsSource);
  }

  function openDetail(item: TicketItem): void {
    void openFrom(projectId, item.project_ids, { kind: 'ticket_detail', ticket: item.ticket.ref });
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
    void startWorkOnTicket(item.ticket.ref, null, { preview, source: item.source_ids[0] ?? null });
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
    const pid = item.project_ids[0] ?? home ?? projects.activeId;
    // No project to open Kelta's review detail in: the browser.
    if (!openPrs(item.ticket.ref.key, item.prs, pid ?? '', browser || !pid)) return;
    const r = (rowEl(keyOf(item)) ?? root)?.getBoundingClientRect();
    prMenu = {
      x: (r?.left ?? 0) + 24,
      y: (r?.bottom ?? 0) + 2,
      prs: item.prs,
      projectId: pid ?? '',
      browser: browser || !pid,
    };
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
    { id: '', label: 'All sources', icon: source === null ? 'check' : undefined },
    ...sources.flatMap((s) => {
      const vs = s.tracker.views;
      const multiAccount = new Set(vs.map((v) => v.account ?? s.tracker.account)).size > 1;
      return [
        { id: `s:${s.id}`, label: s.label, icon: source?.id === s.id && !viewId ? 'check' : undefined },
        // a source's views, indented under it, when there is a choice
        ...(vs.length > 1
          ? vs.map((v) => ({
              id: `v:${s.id}:${v.id}`,
              label: `\u2003${multiAccount ? `${v.label} (${v.account ?? s.tracker.account})` : v.label}`,
              icon: source?.id === s.id && v.id === viewId ? 'check' : undefined,
            }))
          : []),
      ];
    }),
    { id: '+', label: 'Add source…', icon: 'plus', separator: true },
  ]);

  function pickSource(id: string): void {
    if (id === '+') return addSource();
    if (id === '') return setSource(null);
    const [kind, sid = '', ...rest] = id.split(':');
    setSource(sid, kind === 'v' ? rest.join(':') : null);
  }

  function shiftLane(delta: number): void {
    if (!cur || mode !== 'board') return;
    const at = columns.findIndex((c) => c.id === columnFor(columns, cur.ticket.status)?.id);
    if (at < 0) return;
    const target = columns[at + delta];
    if (target) void move.moveToColumn(cur.ticket, target, columnsSource);
  }

  function onkeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, select, [role="dialog"], [role="menu"]')) return;
    // #141: Mod+Enter starts the marked tickets; Space marks on the board only (the list's Space splits).
    const mark =
      mode === 'board' && cur ? { ref: cur.ticket.ref, projectId: cur.project_ids[0] ?? null } : null;
    if (!side?.contains(target) && batchKey(e, mark)) {
      e.preventDefault();
      return;
    }
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
        else if (item) {
          const w = liveWork(item);
          // List, detail closed, work under way: the phase's next step, as in Now. Board: Enter opens.
          if (w && !splitShown && mode === 'list') void runPrimary(w);
          else void focusDetail(item);
        }
        break;
      case ' ':
        if (mode !== 'list') return;
        if (wide) split = !split;
        else if (item) openDetail(item);
        break;
      case '?':
        keysOpen = true;
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
        if (menuChoices) openSourceMenu();
        break;
      case 'g':
        if (item) void focusDetail(item);
        break;
      case 'G':
        setGroup(GROUP_BYS[(GROUP_BYS.indexOf(groupBy) + 1) % GROUP_BYS.length] ?? 'flow');
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
      case 'w':
        if (item) void pickWorkflow(item.ticket.ref);
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
    void move.moveToColumn(item.ticket, column, columnsSource);
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
  <TicketsToolbar
    allProjects={scope.kind === 'all'}
    bind:filter
    bind:filterInput
    onfilterkey={filterKeys}
    hasSource={!noSource}
    who={WHOS.map((w) => {
      const n = countOf(w);
      return { id: w, label: n === undefined ? WHO_LABELS[w] : `${WHO_LABELS[w]} ${n}` };
    })}
    {whoTab}
    onwho={setWho}
    people={[
      { value: '', label: 'Any person' },
      ...(person && !personEntry ? [{ value: person, label: person }] : []),
      ...people.map((p) => ({ value: p.key, label: p.matched ? p.name : `${p.name} (tracker only)` })),
    ]}
    person={personEntry?.key ?? person ?? ''}
    onperson={setPerson}
    showSprint={hasSprints || currentSprint}
    {currentSprint}
    onsprint={() => (currentSprint = !currentSprint)}
    sourceText={menuChoices
      ? source
        ? [source.label, source.tracker.views.find((v) => v.id === viewId)?.label].filter(Boolean).join(' · ')
        : 'All sources'
      : null}
    bind:sourceBtn
    sourceOpen={sourceMenu !== null}
    onsource={openSourceMenu}
    list={mode === 'list'}
    {groupBy}
    ongroup={setGroup}
    {sort}
    onsort={setSort}
    canBoard={single !== null}
    {mode}
    onmode={setMode}
    onrefresh={refresh}
  />

  {#if noSource}
    <EmptyState icon="ticket" title="Add a ticket source: pick a board">
      {#snippet actions()}<Button variant="primary" onclick={addSource}>Pick a board</Button>{/snippet}
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
        <EmptyState icon="ticket" title={`Nothing assigned to you on ${sourceLabel}.`}>
          {#snippet actions()}<Button onclick={() => setWho('unassigned')}>Show unassigned</Button><Button
              onclick={() => setWho('anyone')}>Show team</Button
            >{/snippet}
        </EmptyState>
      {:else if whoTab === 'unassigned'}
        <EmptyState icon="ticket" title={`Every ticket on ${sourceLabel} has an owner.`}>
          {#snippet actions()}<Button onclick={() => setWho('anyone')}>Show team</Button>{/snippet}
        </EmptyState>
      {:else}
        <EmptyState icon="ticket" title={`No open tickets on ${sourceLabel}.`}>
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
      <div class="k-split">
        <div class="list" style:--key-chars={keyChars}>
          <VirtualList
            bind:this={vlist}
            items={rows}
            itemHeight={ROW_HEIGHT}
            key={(r) => r.key}
            label="Tickets"
            onend={() =>
              void tickets
                .loadMore(scope, viewPick, who, sourcePick)
                .catch((e) => toasts.error(e, 'Loading more tickets'))}
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
                <TicketRow
                  item={r.item}
                  rowKey={r.key}
                  selected={cur === r.item}
                  picked={picked.includes(r.key)}
                  picking={picked.length > 0}
                  phase={phaseFor(r.item)}
                  hasWork={hasWork(r.item)}
                  marked={batch.has(r.item.ticket.ref)}
                  {groupBy}
                  source={showSource ? sourceOf(r.item) : null}
                  showAssignee={whoTab !== 'mine'}
                  onselect={(e) => {
                    selKey = r.key;
                    if (e.metaKey || e.ctrlKey) togglePick(r.key);
                  }}
                  onopen={() => openDetail(r.item)}
                  onmove={() => {
                    selKey = r.key;
                    pickOrMove(r.item);
                  }}
                  onpr={() => openPrOf(r.item, false)}
                  onstart={() => start(r.item)}
                />
              {/if}
            {/snippet}
          </VirtualList>
        </div>
        {#if splitShown}
          <aside class="k-split-side" bind:this={side} aria-label="Ticket detail">
            {#if cur}
              <TicketDetail item={cur} projectId={cur.project_ids[0] ?? home} embedded />
            {:else}
              <p class="k-split-empty">Select a ticket to see it here.</p>
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
                    <Lamp level={phaseFor(item)?.lamp ?? 'none'} title={phaseFor(item)?.label} />
                    {#if hasWork(item)}<Badge tone="accent" title="Local work in progress">work</Badge>{/if}
                    {#if batch.has(item.ticket.ref)}<Badge
                        tone="accent"
                        title="Marked: Mod+Enter starts the marked tickets">marked</Badge
                      >{/if}
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
              void tickets
                .loadMore(scope, viewPick, who, sourcePick)
                .catch((e) => toasts.error(e, 'Loading more tickets'))}
          >
            <Icon name="plus" size={12} /> More
          </Button>
        {/if}
      </div>
    {/if}
  {/if}

  <KeyHints hints={[...hints.slice(0, 5), ['?', 'All keys']]} />
</div>

{#if picker}
  <StatusPicker tickets={picker.tickets} projectId={home} anchor={picker.anchor} onclose={closePicker} />
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
    onselect={pickSource}
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
{#if keysOpen}
  <Dialog
    title="Ticket keys"
    width={420}
    onclose={() => {
      keysOpen = false;
      root?.focus();
    }}
  >
    <dl class="keys">
      {#each hints as [keys, label] (keys)}
        <dt>
          {#each keys.split(' ') as k (k)}{#if k === 'then'}<span class="then">then</span>{:else}<Kbd
                chord={k}
              />{/if}{/each}
        </dt>
        <dd>{label}</dd>
      {/each}
    </dl>
  </Dialog>
{/if}

<style>
  .keys {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--k-space-3) var(--k-space-5);
    margin: 0;
    align-items: center;
  }

  .keys dt {
    display: inline-flex;
    gap: var(--k-space-1);
    justify-self: end;
  }

  .keys dd {
    margin: 0;
    color: var(--k-fg-muted);
  }

  .then {
    padding: 0 2px;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
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
