<script lang="ts">
  // Now (FLOW §3): one screen across projects answering "review Claude's work, or grab a feature?".
  // It keeps the Inbox slot (rail top tile, ⌘0, `inbox.open`, pane kind `inbox`).
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { tick, untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError, RowDensity, SessionInfo, TicketRef } from '$lib/gen';
  import { openExternal } from '$lib/ipc/commands';
  import {
    GLOBAL,
    layout,
    projects,
    reviews,
    sessions,
    settings,
    tickets,
    toasts,
    ui,
    work,
  } from '$lib/stores';
  import { inflight } from '$lib/stores/inflight.svelte';
  import { reviewKey } from '$lib/stores/reducers';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import type { TagKind } from '$lib/stores/tags.svelte';
  import { resolveProjectColor } from '$lib/theme';
  import {
    Button,
    EmptyState,
    ErrorState,
    Icon,
    IconButton,
    Kbd,
    Lamp,
    LoadBar,
    Menu,
    ROW_HEIGHT,
    Spinner,
    Toggle,
    VirtualList,
    relativeTime,
  } from '$lib/ui';

  import { chordFor } from '../../shell/labels';
  import { openView } from '../../shell/nav';
  import { peekHover } from '../../shell/peek.svelte';
  import { ctxHot, itemCost, overBudget, usd } from '../../shell/usage';
  import { blockedReason, runWorkAction, workAction, WORK_ACTIONS } from '../work/actions';
  import { batch, batchKey } from '../work/batch.svelte';
  import { isAuthError } from '../work/common';
  import AskBar from '../work/AskBar.svelte';
  import { askerOf, claudeOf, prOf, sessionLabel, workTitle } from '../work/live';
  import { loadGlobalLayout, persistPaneContent } from '../work/nav';
  import {
    deltaParts,
    outsidePhase,
    questionOf,
    reviewOf,
    testsMissing,
    workKey,
    type Lamp as LampKind,
    type NowSection,
    type WorkActionId,
  } from '../work/phase';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { isNextUp, SECTION_HINTS, type NowRow, type Section } from './groups';
  import { asOf, enterRow, goToRow, nowSummary, refreshNow, reviewRowLocally, startNextUp } from './now';
  import { nextUp } from './nextUp.svelte';
  import { waitChip } from '../people/waiting';
  import TagChips from '../tags/TagChips.svelte';
  import { waitingChip } from '../reviews/nudge';
  import FilterBar from '../filters/FilterBar.svelte';
  import { clearAll, commonFilters, noMatch } from '../filters/filters';

  let { projectId, paneId, content, focused }: PaneProps<'inbox'> = $props();

  let densityOverride = $state<RowDensity | null>(null);
  /** Compact keeps the single 32px line (#383); comfortable is the two-line row. */
  const compact = $derived((densityOverride ?? content.density) === 'compact');

  function setCompact(on: boolean): void {
    densityOverride = on ? 'compact' : 'comfortable';
    persistPaneContent(projectId, paneId, { density: densityOverride });
  }

  const ALL = { kind: 'all' } as const;

  $effect(() => {
    untrack(() => void refreshNow());
  });
  // once at mount, not in an $effect: load() writes the state value() reads (a loop while pending)
  if (settings.value() === null) void settings.load().catch(() => undefined);

  /** No ticket source at all: Now says so and offers the source picker. */
  const noSource = $derived(settings.hasSources() === false);
  const pickBoard = () => ui.openSheet('tracker.source_picker', {});

  function refresh(): void {
    void refreshNow(true);
  }

  let filter = $state('');
  let filtering = $state(false);
  let filterEl = $state<HTMLInputElement>();

  const summary = $derived(nowSummary());
  const stale = $derived(asOf());
  const lists = $derived([
    tickets.list(ALL, null, 'mine'),
    reviews.list(ALL, 'review_requested'),
    reviews.list(ALL, 'authored'),
  ]);
  const noData = $derived(lists.every((l) => !l.data) && work.all.length === 0);
  const loading = $derived(lists.some((l) => l.loading) && noData);
  const firstError = $derived(lists.find((l) => l.error)?.error ?? null);
  const accountErrors = $derived.by(() => {
    const seen: Record<string, AccountError> = {};
    for (const l of lists) for (const e of l.data?.errors ?? []) seen[`${e.account_id}:${e.error.code}`] = e;
    return Object.values(seen);
  });

  interface View {
    lamp: LampKind;
    id: string;
    title: string;
    project: string | null;
    color: string | null;
    reason: string;
    meta: string[];
    /** Ticket and review rows: where their local tags live (#349). */
    tagged?: { kind: TagKind; key: string };
    /** Work rows: Claude context use and spend. */
    ctx?: { text: string; hot: boolean } | null;
    cost?: { text: string; over: boolean } | null;
    /** The meta part shown as a warning (`tests: none` when source changed). */
    warn?: string;
    age: string;
    /** Second line of the selected row. */
    more: string;
    /** The selected row's buttons (#397), the key on each; the `enter` one is the primary. */
    actions: { key: string; label: string; run: () => unknown }[];
    /** A Claude session Kelta did not start: shown dimmed. */
    dim?: boolean;
  }

  function projectOf(ids: readonly string[]): { name: string | null; color: string | null } {
    const p = ids[0] ? projects.byId(ids[0]) : null;
    return { name: p?.name ?? null, color: p?.color ?? null };
  }

  const age = (iso: string): string => relativeTime(Date.parse(iso));

  // Work row letters in Now (FLOW §3.3); `o` opens the PR, else the ticket. `R` marks reviewed on a
  // row that has something to review, else refreshes.
  const NOW_KEYS: Partial<Record<string, WorkActionId>> = {
    v: 'review_delta',
    V: 'review_diff',
    R: 'mark_reviewed',
    b: 'edit_note',
    p: 'ship',
    f: 'fix',
    r: 'rebase',
    c: 'rebase_continue',
    a: 'rebase_abort',
    n: 'conflicts',
    s: 'skip_step',
    P: 'park',
  };

  function view(row: NowRow): View {
    switch (row.type) {
      case 'work': {
        const { item, phase } = row;
        const git = work.git[item.id];
        const pr = prOf(item);
        const meta: string[] = row.blocked ? [t('Blocked')] : [];
        const delta = phase.section === 'to_review' ? item.delta : null;
        if (delta) meta.push(...deltaParts(delta));
        else if (git && (git.insertions || git.deletions))
          meta.push(`+${git.insertions} −${git.deletions}${git.dirty ? ` ${t('uncommitted')}` : ''}`);
        else if (pr) meta.push(`#${pr.ref.number}`);
        if (git?.behind) meta.push(t('{n} behind {base}', { n: git.behind, base: item.base }));
        if (item.auto_finish) meta.push(t('armed'));
        if (phase.lamp === 'parked' && item.parked_at)
          meta.push(t('parked {ago}', { ago: age(item.parked_at) }));
        if (item.nvim_kept)
          meta.push(tn('nvim kept: {n} unsaved buffer', 'nvim kept: {n} unsaved buffers', item.nvim_kept));
        const p = projectOf([item.project_id]);
        const actions = [
          ...(phase.primary ? [{ key: 'enter', label: phase.primaryLabel, run: () => enterRow(row) }] : []),
          ...WORK_ACTIONS.filter(
            (a) =>
              a.key && Object.values(NOW_KEYS).includes(a.id) && blockedReason(a.id, item, phase) === null,
          ).map((a) => ({
            key: a.key!,
            label: a.label({ item, phase }),
            run: () => runWorkAction(a.id, item),
          })),
          ...(item.claude_message && phase.section === 'to_review'
            ? [
                {
                  key: 'm',
                  label: expanded === row.id ? t('Hide message') : t('Full message'),
                  run: () => (expanded = expanded === row.id ? null : row.id),
                },
              ]
            : []),
          { key: 'o', label: t('Open in browser'), run: () => browse(row) },
          { key: 'g', label: t('Go to work tab'), run: () => goToRow(row) },
        ];
        const claude = claudeOf(item)?.claude;
        const u = claude?.usage;
        const spent = itemCost(item, sessions.all);
        const preview = claude?.preview ?? '';
        const said = item.claude_message?.trim().split('\n')[0] || preview;
        const more =
          phase.section === 'to_review'
            ? [item.next_note ? t('next: {note}', { note: item.next_note }) : '', said]
                .filter(Boolean)
                .join('  ·  ') || phase.detail
            : phase.section === 'needs_you' && preview
              ? preview
              : phase.id === 'checks_failed' && pr?.failed_check_names.length
                ? `#${pr.ref.number} · ${pr.failed_check_names.join(', ')}`
                : phase.detail;
        return {
          lamp: phase.lamp,
          id: workKey(item),
          title: workTitle(item),
          project: p.name,
          color: p.color,
          reason: row.status
            ? `${row.status} · ${phase.label}`
            : phase.section === 'needs_you' && phase.detail
              ? phase.detail
              : phase.label,
          meta,
          ctx:
            u && u.context_pct !== null
              ? { text: `${t('ctx')} ${Math.round(u.context_pct)}%`, hot: ctxHot(u) }
              : null,
          cost:
            spent > 0
              ? {
                  text: usd(spent),
                  over: overBudget(spent, settings.value(item.project_id)?.claude.budget_usd),
                }
              : null,
          warn: delta && testsMissing(delta) ? t('tests: none') : row.blocked ? t('Blocked') : undefined,
          // Ready for review: how long it has waited since Claude stopped.
          age: age(delta ? (item.claude_at ?? item.created_at) : item.created_at),
          more,
          actions,
        };
      }
      case 'session': {
        const s = row.session;
        const p = projectOf([s.project_id]);
        const preview = s.claude?.preview ?? '';
        return {
          lamp: 'needs_input',
          id: s.kind.type,
          title: sessionLabel(s),
          project: p.name,
          color: p.color,
          reason: preview || t('Needs input'),
          meta: [],
          age: age(s.created_at),
          more: preview,
          actions: [{ key: 'enter', label: t('Go to session'), run: () => enterRow(row) }],
        };
      }
      case 'outside': {
        const s = row.session;
        const ph = outsidePhase(s);
        return {
          lamp: ph.lamp,
          id: s.ticket?.key ?? 'claude',
          title: s.title,
          project: null,
          color: null,
          reason: ph.label,
          meta: s.branch ? [s.branch] : [],
          age: age(s.updated_at),
          more: [s.cwd, s.preview].filter(Boolean).join('  ·  '),
          actions: [],
          dim: true,
        };
      }
      case 'review': {
        const r = row.review.review;
        const p = projectOf(row.review.project_ids);
        const stat = r.additions !== null && r.deletions !== null ? [`+${r.additions} −${r.deletions}`] : [];
        const chip = waitingChip(row.review, settings.value()?.reviews.sla_hours ?? 24);
        return {
          lamp: 'none',
          id: `#${r.ref.number}`,
          title: r.title,
          project: p.name ?? r.ref.repo,
          color: p.color,
          // A plain request reads as who asked; mine and "Updated since your review" read as the reason.
          // Red CI does not hide a pending review (#369): `Checks failed · review required`.
          reason:
            !row.mine && row.reason === 'Review requested'
              ? r.author.name
              : [t(row.reason), row.reason === 'Checks failed' ? reviewOf(r) : '']
                  .filter(Boolean)
                  .join(' · '),
          tagged: { kind: 'prs', key: reviewKey(r.ref) },
          meta: [...stat, ...(row.mine && chip ? [chip.label] : [])],
          age: age((!row.mine && r.requested_at) || r.updated_at),
          more: `${r.source_branch} ${t('into')} ${r.target_branch}${
            r.failed_check_names.length
              ? `, ${t('checks failed: {names}', { names: r.failed_check_names.join(', ') })}`
              : r.ci === 'failure'
                ? `, ${t('checks failed')}`
                : ''
          }`,
          actions: [
            { key: 'enter', label: t('Open review'), run: () => enterRow(row) },
            ...(row.mine ? [] : [{ key: 's', label: t('Review locally'), run: () => reviewRowLocally(row) }]),
            { key: 'o', label: t('Open on host'), run: () => browse(row) },
          ],
        };
      }
      case 'ticket': {
        const tk = row.ticket.ticket;
        const p = projectOf(row.ticket.project_ids);
        return {
          lamp: 'none',
          id: tk.ref.key,
          title: tk.title,
          project: p.name,
          color: p.color,
          reason: tk.status.name,
          tagged: { kind: 'tickets', key: ticketKey(tk.ref) },
          meta: [
            ...(tk.blocked ? [t('Blocked')] : []),
            ...(nextUp.isNew(tk.ref) ? [t('New')] : []),
            ...(batch.has(tk.ref) ? [t('marked')] : []),
            ...(tk.priority ? [tk.priority] : []),
          ],
          warn: tk.blocked ? t('Blocked') : undefined,
          age: age(tk.updated_at),
          more: waitChip(row.ticket.waiting, { withWhy: true })?.label ?? '',
          actions: [
            { key: 'enter', label: t('Start work'), run: () => enterRow(row) },
            ...(isNextUp(row)
              ? [
                  { key: 'shift+s', label: t('Start the top ones'), run: () => startNextUp(nextUpRows()) },
                  { key: 'shift+j', label: t('Down'), run: () => reorder(row, 1) },
                  { key: 'shift+k', label: t('Up'), run: () => reorder(row, -1) },
                ]
              : []),
            {
              key: 'space',
              label: batch.has(tk.ref) ? t('Unmark') : t('Mark ticket'),
              run: () => batch.toggle(tk.ref, row.ticket.project_ids[0] ?? null),
            },
            ...(batch.list.length > 0
              ? [
                  {
                    key: chordFor('work.start') ?? 'mod+enter',
                    label: t('Start marked'),
                    run: () => ui.openSheet('start_batch'),
                  },
                ]
              : []),
            { key: 'o', label: t('Open in tracker'), run: () => browse(row) },
          ],
        };
      }
    }
  }

  function matches(row: NowRow, q: string): boolean {
    if (!q) return true;
    const v = view(row);
    const branch =
      row.type === 'work' ? row.item.branch : row.type === 'review' ? row.review.review.source_branch : '';
    return `${v.id} ${v.title} ${v.project ?? ''} ${branch}`.toLowerCase().includes(q.toLowerCase());
  }

  type Line =
    | { type: 'section'; id: string; section: Section; count: number; open: boolean }
    | { type: 'row'; id: string; row: NowRow }
    | { type: 'detail'; id: string; row: NowRow }
    /** The option buttons of the row's question (#356). */
    | { type: 'ask'; id: string; session: SessionInfo }
    | { type: 'msg'; id: string; text: string }
    | { type: 'more'; id: string; count: number };

  let selId = $state<string | null>(null);
  /** Row whose full Claude message is unfolded under its detail line (`m`). */
  let expanded = $state<string | null>(null);
  /** Folded sections (Waiting to deploy) opened by their header. */
  let unfolded = $state<string[]>([]);
  const isOpen = (s: Section): boolean => !s.folded || unfolded.includes(s.id);
  const toggleSection = (s: Section): void => {
    unfolded = isOpen(s) ? unfolded.filter((id) => id !== s.id) : [...unfolded, s.id];
  };

  const visible = $derived(
    summary.sections
      .map((s) => ({ ...s, rows: s.rows.filter((r) => matches(r, filter.trim())) }))
      .filter((s) => s.rows.length > 0 || s.more > 0),
  );
  const activeFilters = $derived(commonFilters(filter, () => (filter = '')));
  const selectable = $derived(visible.filter(isOpen).flatMap((s) => s.rows));
  const cur = $derived(selectable.find((r) => r.id === selId) ?? selectable[0] ?? null);

  const lines = $derived.by((): Line[] => {
    const out: Line[] = [];
    for (const s of visible) {
      const open = isOpen(s);
      out.push({ type: 'section', id: `s:${s.id}`, section: s, count: s.rows.length + s.more, open });
      if (!open) continue;
      for (const row of s.rows) {
        out.push({ type: 'row', id: row.id, row });
        const asker = rowAsker(row);
        if (asker) out.push({ type: 'ask', id: `a:${row.id}`, session: asker });
        if (row === cur) out.push({ type: 'detail', id: `d:${row.id}`, row });
        if (row === cur && expanded === row.id && row.type === 'work')
          (row.item.claude_message ?? '')
            .split('\n')
            .forEach((text, i) => out.push({ type: 'msg', id: `m:${row.id}:${i}`, text }));
      }
      if (s.more > 0) out.push({ type: 'more', id: `m:${s.id}`, count: s.more });
    }
    return out;
  });

  let vlist = $state<{ scrollToIndex(i: number): void }>();
  let root = $state<HTMLDivElement>();

  $effect(() => {
    if (cur) vlist?.scrollToIndex(lines.findIndex((l) => l.type === 'row' && l.row === cur));
  });

  $effect(() => {
    if (!focused || cur?.type !== 'ticket') return;
    selectTicket(cur.ticket.ticket.ref, cur.ticket.project_ids[0] ?? null);
    return () => selectTicket(null, null);
  });

  $effect(() => {
    if (focused && !filtering && root && !root.contains(document.activeElement))
      root.focus({ preventScroll: true });
  });

  /** A header chip: selects its section's first row and brings the section heading into view. */
  async function jumpTo(id: NowSection): Promise<void> {
    const first = visible.find((s) => s.id === id)?.rows[0];
    if (!first) return;
    selId = first.id;
    root?.focus({ preventScroll: true });
    await tick();
    vlist?.scrollToIndex(lines.findIndex((l) => l.type === 'section' && l.section.id === id));
  }

  function step(delta: number): void {
    if (selectable.length === 0) return;
    const at = cur ? selectable.indexOf(cur) : -1;
    const next = selectable[Math.min(selectable.length - 1, Math.max(0, at + delta))];
    if (next) selId = next.id;
  }

  function browse(row: NowRow): void {
    const url =
      row.type === 'ticket'
        ? row.ticket.ticket.url
        : row.type === 'review'
          ? row.review.review.url
          : row.type === 'work'
            ? (row.item.pr_url ?? prOf(row.item)?.url ?? null)
            : null;
    if (row.type === 'ticket') nextUp.markSeen(row.ticket.ticket.ref);
    if (url) openExternal({ url }).catch((err) => toasts.error(err, t('Open in browser')));
    else if (row.type === 'work') void runWorkAction('open_ticket', row.item);
  }

  function letter(row: NowRow, key: string): void {
    if (key === 'o') return browse(row);
    if (row.type === 'work') {
      const id = NOW_KEYS[key];
      if (id) void inflight.track(row.id, () => runWorkAction(id, row.item));
      return;
    }
    if (key === 's' && row.type === 'ticket') void inflight.track(row.id, () => enterRow(row));
    else if (key === 's' && row.type === 'review' && !row.mine) reviewRowLocally(row);
  }

  /** Next up as shown (unfiltered): `S` starts its top ones, `J`/`K` reorder within it. */
  const nextUpRows = (): NowRow[] => summary.sections.find((s) => s.id === 'next_up')?.rows ?? [];

  const nextUpRefs = (): TicketRef[] =>
    nextUpRows().flatMap((r) => (r.type === 'ticket' ? [r.ticket.ticket.ref] : []));

  /** `J` / `K` and the Down / Up buttons: one place in Next up. */
  function reorder(row: NowRow, delta: 1 | -1): void {
    if (row.type === 'ticket') void nextUp.move(row.ticket.ticket.ref, delta, nextUpRefs());
  }

  // ---- drag to reorder Next up (#397) --------------------------------------------------------
  let dragId = $state<string | null>(null);
  let overId = $state<string | null>(null);

  function drop(target: NowRow): void {
    const from = summary.sections.find((s) => s.id === 'next_up')?.rows.find((r) => r.id === dragId);
    dragId = overId = null;
    if (from?.type !== 'ticket' || target.type !== 'ticket' || from === target) return;
    const refs = nextUpRefs();
    const to = refs.findIndex((r) => ticketKey(r) === ticketKey(target.ticket.ticket.ref));
    void nextUp.moveTo(from.ticket.ticket.ref, to, refs);
  }

  /** Up next's "Show all": the rail's Tickets view, on Mine. */
  async function showAll(): Promise<void> {
    // The layout may have failed to load at startup: without it the patch would be dropped.
    if (!layout.get(GLOBAL)) await loadGlobalLayout().catch(() => null);
    persistPaneContent(GLOBAL, 'tickets', { who: 'mine', person: null });
    openView('tickets');
  }

  /** The full key list (`?`); the footer shows the first five. */
  let keysOpen = $state(false);
  const hints = $derived<[string, string][]>([
    ['j k', msg('Move')],
    ['enter', msg('Next action')],
    ['g', msg('Go to')],
    ['/', msg('Filter')],
    ['space', msg('Mark ticket')],
    ['o', msg('Open in browser')],
    ['shift+r', msg('Refresh')],
    [chordFor('work.start') ?? 'mod+enter', msg('Start marked')],
  ]);

  function onkeydown(e: KeyboardEvent): void {
    // A section header keeps its own keys (Enter, Space); they must not act on the selected row.
    if (e.target instanceof HTMLButtonElement && e.target.classList.contains('fold')) return;
    const ticket =
      cur?.type === 'ticket'
        ? { ref: cur.ticket.ticket.ref, projectId: cur.ticket.project_ids[0] ?? null }
        : null;
    if (!filtering && batchKey(e, ticket)) {
      e.preventDefault();
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey || filtering) return;
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
        if (selectable[0]) selId = selectable[0].id;
        break;
      case 'End':
        if (selectable.at(-1)) selId = selectable.at(-1)!.id;
        break;
      case 'Enter':
        if (cur) {
          const row = cur;
          void inflight.track(row.id, () => enterRow(row));
        }
        break;
      case 'g':
        if (cur) void goToRow(cur);
        break;
      case '?':
        keysOpen = true;
        break;
      case '/':
        filtering = true;
        queueMicrotask(() => filterEl?.focus());
        break;
      case 'R':
        if (cur?.type === 'work' && blockedReason('mark_reviewed', cur.item) === null) letter(cur, 'R');
        else refresh();
        break;
      case 'm':
        if (cur?.type === 'work' && cur.item.claude_message) expanded = expanded === cur.id ? null : cur.id;
        break;
      case 'N':
        void dispatch('work.new');
        break;
      case 'S':
        if (cur && isNextUp(cur)) startNextUp(nextUpRows());
        break;
      case 'J':
      case 'K':
        if (cur && isNextUp(cur)) reorder(cur, e.key === 'J' ? 1 : -1);
        break;
      default:
        if (!cur || !'vVbpfrcansoP'.includes(e.key) || e.key.length !== 1) return;
        letter(cur, e.key);
    }
    e.preventDefault();
  }

  // Right-click on a queued row: Start now (over cap), Move to front.
  const QUEUE_MENU: WorkActionId[] = ['start_now', 'queue_front'];
  let queueMenu = $state<{ x: number; y: number; row: NowRow & { type: 'work' } } | null>(null);

  /** The session a Now row peeks at: its own, or the work item's asking session or Claude. */
  function rowSession(row: NowRow): string | null {
    if (row.type === 'session') return row.session.id;
    return row.type === 'work' ? ((askerOf(row.item) ?? claudeOf(row.item))?.id ?? null) : null;
  }

  function rowAsker(row: NowRow): SessionInfo | null {
    if (row.type === 'session') return questionOf(row.session) ? row.session : null;
    return row.type === 'work' ? askerOf(row.item) : null;
  }

  function rowMenu(e: MouseEvent, row: NowRow): void {
    if (row.type !== 'work' || row.item.state.kind !== 'queued') return;
    e.preventDefault();
    selId = row.id;
    queueMenu = { x: e.clientX, y: e.clientY, row };
  }

  function onFilterKey(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      filter = '';
      filtering = false;
      root?.focus();
    } else if (e.key === 'Enter' || e.key === 'ArrowDown') {
      filtering = false;
      root?.focus();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane"
  class:compact
  data-testid="inbox-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={t('Now')}
  {onkeydown}
>
  <header class="bar">
    <span class="heading">
      <span class="title">{t('Now')}</span>
      <span class="subtitle">{t('Everything waiting on you, across projects')}</span>
    </span>
    <span class="split" data-testid="now-header" title={summary.header}>
      {#each summary.chips as c (c.section)}
        {@const at = c.text.indexOf(String(c.n))}
        <Button
          size="sm"
          variant="ghost"
          class="count {c.lamp ?? ''}"
          disabled={!visible.some((s) => s.id === c.section && s.rows.length > 0)}
          data-testid="now-chip-{c.section}"
          onclick={() => void jumpTo(c.section)}
          >{#if at < 0}{c.text}{:else}{c.text.slice(0, at)}<b class="k-num">{c.n}</b>{c.text.slice(
              at + String(c.n).length,
            )}{/if}</Button
        >
      {/each}
    </span>
    {#if stale}<span class="asof" data-testid="now-asof"
        >{t('Updated {ago}', { ago: relativeTime(stale.ms) })}</span
      >{/if}
    <span class="spacer"></span>
    <Toggle label={t('Compact')} checked={compact} onchange={setCompact} />
    <IconButton
      icon="plus"
      size="sm"
      label={t('New work item')}
      chord={chordFor('work.new')}
      onclick={() => void dispatch('work.new')}
    />
    <Button variant="ghost" size="sm" icon="refresh-cw" chord="shift+r" onclick={refresh}
      >{t('Refresh')}</Button
    >
  </header>
  <FilterBar
    bind:value={filter}
    bind:input={filterEl}
    active={activeFilters}
    placeholder={t('Filter id, title, project, branch (/)')}
    aria-label={t('Filter Now')}
    onfocus={() => (filtering = true)}
    onkeydown={onFilterKey}
    onblur={() => (filtering = false)}
  />
  <LoadBar active={!loading && lists.some((l) => l.loading)} label={t('Loading Now')} />

  {#if loading}
    <Loading label={t('Loading Now')} />
  {:else if noData && firstError}
    <ErrorState error={firstError} title={t('Could not load Now')} onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
          {isAuthError(firstError) ? t('Re-authenticate') : t('Open account settings')}
        </Button>
      {/snippet}
    </ErrorState>
  {:else}
    <StateBanner error={noData ? null : firstError} errors={accountErrors} onretry={refresh} />
    {#if noSource && lines.length > 0}
      <div class="nosource" data-testid="now-no-source">
        {t('No ticket source yet')} ·
        <button type="button" class="k-link" onclick={pickBoard}>{t('Pick a board')}</button>
      </div>
    {/if}
    {#if lines.length === 0 && noSource && activeFilters.length === 0}
      <EmptyState icon="ticket" title={t('Add a ticket source: pick a board')}>
        {#snippet actions()}<Button variant="primary" onclick={pickBoard}>{t('Pick a board')}</Button
          >{/snippet}
      </EmptyState>
    {:else if lines.length === 0}
      <EmptyState
        icon={activeFilters.length > 0 ? 'search' : 'inbox'}
        title={activeFilters.length > 0 ? noMatch(activeFilters) : t('Nothing needs you right now')}
        body={activeFilters.length > 0
          ? ''
          : t(
              'Claude sessions that ask a question, pull requests waiting for your review and tickets assigned to you show up here.',
            )}
      >
        {#snippet actions()}
          {#if activeFilters.length > 0}<Button onclick={() => clearAll(activeFilters)}
              >{t('Clear all')}</Button
            >{:else}<Button onclick={refresh}>{t('Refresh')}</Button>{/if}
        {/snippet}
      </EmptyState>
    {:else}
      <div class="list">
        <VirtualList
          bind:this={vlist}
          items={lines}
          itemHeight={ROW_HEIGHT}
          measure={!compact}
          key={(l) => l.id}
          label={t('Now')}
          sticky={(l) => l.type === 'section'}
        >
          {#snippet row(l)}
            {#if l.type === 'section'}
              <div class="section" role="heading" aria-level="2" data-section={l.section.id}>
                {#if l.section.folded}
                  {@const s = l.section}
                  <button type="button" class="fold" aria-expanded={l.open} onclick={() => toggleSection(s)}>
                    <span class="slot chevron"><Icon name="chevron-right" size={12} /></span>
                    {t(s.label)}
                    <span class="count">{l.count}</span>
                    <span class="k-group-hint">{t(SECTION_HINTS[s.id])}</span>
                  </button>
                {:else}
                  <span class="slot"><Lamp level={l.section.lamp} title="" /></span>
                  {t(l.section.label)}
                  <span class="count">{l.count}</span>
                  <span class="k-group-hint">{t(SECTION_HINTS[l.section.id])}</span>
                {/if}
              </div>
            {:else if l.type === 'ask'}
              <div class="row askline"><AskBar session={l.session} /></div>
            {:else if l.type === 'msg'}
              <div class="row msgline" data-testid="now-message">{l.text}</div>
            {:else if l.type === 'more'}
              <button type="button" tabindex="-1" class="row moreline" onclick={() => void showAll()}>
                {t('{n} more, show all', { n: l.count })}
              </button>
            {:else}
              {@const v = view(l.row)}
              {#if l.type === 'row'}
                <button
                  type="button"
                  tabindex="-1"
                  class="row"
                  class:two={!compact}
                  class:dim={v.dim}
                  class:selected={cur === l.row}
                  class:over={overId === l.row.id && dragId !== l.row.id}
                  aria-current={cur === l.row ? 'true' : undefined}
                  data-row={l.row.id}
                  draggable={isNextUp(l.row) ? 'true' : undefined}
                  ondragstart={(e) => {
                    dragId = l.row.id;
                    selId = l.row.id;
                    e.dataTransfer?.setData('text/plain', l.row.id);
                  }}
                  ondragover={(e) => {
                    if (dragId && isNextUp(l.row)) {
                      e.preventDefault();
                      overId = l.row.id;
                    }
                  }}
                  ondragleave={() => (overId = null)}
                  ondrop={(e) => {
                    e.preventDefault();
                    drop(l.row);
                  }}
                  ondragend={() => (dragId = overId = null)}
                  style:--hue={resolveProjectColor(v.color) ?? 'transparent'}
                  onclick={() => (selId = l.row.id)}
                  ondblclick={() => void enterRow(l.row)}
                  oncontextmenu={(e) => rowMenu(e, l.row)}
                  use:peekHover={() => rowSession(l.row)}
                >
                  <span class="slot"
                    >{#if inflight.has(l.row.id)}<Spinner size={10} />{:else}<Lamp
                        level={v.lamp}
                        title={v.reason}
                      />{/if}</span
                  >
                  <span class="id">{v.id}</span>
                  <span class="ttl">{v.title}</span>
                  <span class="sub">
                    {#if v.tagged}<TagChips kind={v.tagged.kind} itemKey={v.tagged.key} />{/if}
                    {#if v.project}<span class="proj">{v.project}</span>{/if}
                    <span class="reason">{v.reason}</span>
                    {#each v.meta as m (m)}<span class="meta" class:warn={m === v.warn}>{m}</span>{/each}
                    {#if v.ctx}<span
                        class="meta"
                        class:hot={v.ctx.hot}
                        title={v.ctx.hot ? t('Compacts soon') : t('Context window used')}>{v.ctx.text}</span
                      >{/if}
                    {#if v.cost}<span
                        class="meta"
                        class:over={v.cost.over}
                        title={v.cost.over ? t('Over budget') : t('Claude spend on this item')}
                        >{v.cost.text}</span
                      >{/if}
                    <span class="meta" class:age={compact}>{v.age}</span>
                  </span>
                </button>
              {:else}
                <div
                  class="row detail"
                  style:--hue={resolveProjectColor(v.color) ?? 'transparent'}
                  data-testid="now-detail"
                >
                  {#each v.actions as a (a.key)}
                    <button
                      type="button"
                      tabindex="-1"
                      class="act"
                      class:primary={a.key === 'enter'}
                      data-key={a.key}
                      onclick={() => void inflight.track(l.row.id, a.run)}
                      ><Kbd chord={a.key} />{a.label}</button
                    >
                  {/each}
                  {#if v.more}<span class="msg" title={v.more}>{v.more}</span>{/if}
                </div>
              {/if}
            {/if}
          {/snippet}
        </VirtualList>
      </div>
    {/if}
  {/if}

  <KeyHints {hints} onopen={() => (keysOpen = true)} />
</div>

{#if keysOpen}
  <KeyHints
    {hints}
    onclose={() => {
      keysOpen = false;
      root?.focus();
    }}
  />
{/if}
{#if queueMenu}
  {@const item = queueMenu.row.item}
  <Menu
    items={QUEUE_MENU.map((id) => ({
      id,
      label: workAction(id).label({ item, phase: queueMenu!.row.phase }),
    }))}
    x={queueMenu.x}
    y={queueMenu.y}
    label={t('Queued work')}
    onselect={(id) => void runWorkAction(id as WorkActionId, item)}
    onclose={() => {
      queueMenu = null;
      root?.focus();
    }}
  />
{/if}

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
    gap: var(--k-space-4);
    min-height: var(--k-toolbar-height);
    padding: 0 var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
    background: var(--k-bg-elev);
  }

  .heading {
    display: flex;
    flex-direction: column;
    flex: none;
    padding: var(--k-space-2) 0;
    line-height: 1.3;
  }

  .title {
    font-weight: 600;
  }

  .split {
    display: inline-flex;
    gap: var(--k-space-1);
    min-width: 0;
    overflow: hidden;
    font-size: var(--k-font-size-sm);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-muted);
    white-space: nowrap;
  }

  .split :global(.count) {
    color: var(--k-fg-muted);
  }

  .split :global(.count b) {
    font-weight: 600;
  }

  /* Only the sections that need Louis take their lamp colour. */
  .split :global(.count.needs_input) {
    color: var(--k-lamp-needs-input);
  }

  .split :global(.count.error) {
    color: var(--k-lamp-error);
  }

  .subtitle {
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .asof {
    font-size: var(--k-font-size-xs);
    color: var(--k-warn);
  }

  .spacer {
    flex: 1;
  }

  .list {
    flex: 1;
    min-height: 0;
  }

  .section {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    height: 100%;
    padding: 0 var(--k-space-3) 0 var(--k-space-4);
    font-size: var(--k-font-size-sm);
    font-weight: 600;
    color: var(--k-fg-muted);
    background: var(--k-bezel-raised);
    box-shadow: inset 0 -1px 0 var(--k-border);
  }

  /* The chevron turns as the section folds or opens; the rows fade in and out (VirtualList). */
  .chevron :global(svg) {
    transition: rotate var(--k-duration-fast) var(--k-ease);
  }

  .fold[aria-expanded='true'] .chevron :global(svg) {
    rotate: 90deg;
  }

  .fold {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--k-space-2);
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .count {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
  }

  .slot {
    display: inline-flex;
    justify-content: center;
    flex: none;
    width: 12px;
    margin-right: var(--k-space-2);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    width: 100%;
    min-height: var(--k-row-height);
    padding: 0 var(--k-space-3) 0 var(--k-space-4);
    border: 0;
    box-shadow: inset 3px 0 0 var(--hue);
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--k-font-size-md, 13px);
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--k-bg-hover);
  }

  /* Outside Kelta (#345): the lamp at full strength, the words quieter. */
  .row.dim > :not(.slot) {
    opacity: 0.6;
  }

  .selected,
  .detail {
    background: var(--k-bg-selected-row);
    --k-fg-subtle: var(--k-fg-subtle-sel);
    --k-fg-muted: var(--k-fg-muted-sel);
  }

  .id {
    flex: none;
    width: 76px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    color: var(--k-info);
  }

  .ttl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: var(--k-weight-medium);
    color: var(--k-fg-strong);
  }

  .selected .ttl {
    font-weight: var(--k-weight-strong);
  }

  .proj {
    flex: none;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .reason {
    flex: none;
    max-width: 36ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg);
  }

  .meta {
    flex: none;
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
  }

  .meta.hot {
    color: var(--k-warn);
  }

  .meta.over {
    color: var(--k-danger);
  }

  .meta.warn {
    color: var(--k-warn);
  }

  .nosource {
    padding: var(--k-space-2) var(--k-space-4);
    border-bottom: 1px solid var(--k-border);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .msgline {
    padding-left: calc(var(--k-space-4) + 12px + 76px + 3 * var(--k-space-2));
    background: var(--k-bg-selected-row);
    --k-fg-muted: var(--k-fg-muted-sel);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .askline {
    padding-left: calc(var(--k-space-4) + 12px + 76px + 3 * var(--k-space-2));
  }

  /* Two-line row (#383): lamp, id and title on line 1; the meta under the title, wrapping rather
     than cut. */
  .two {
    display: grid;
    grid-template-columns: auto auto minmax(0, 1fr);
    column-gap: var(--k-space-2);
    row-gap: var(--k-space-1);
    padding-block: var(--k-space-2);
  }

  /* Compact: the meta joins the row's own line. */
  .row:not(.two) .sub {
    display: contents;
  }

  .age {
    width: 64px;
    text-align: right;
  }

  .sub {
    grid-column: 3;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-1) var(--k-space-3);
    min-width: 0;
  }

  /* The selected row's key hints, then its message, under the title; hints wrap rather than cut. */
  .detail {
    flex-wrap: wrap;
    gap: var(--k-space-1) var(--k-space-3);
    padding-block: 0 var(--k-space-2);
    padding-left: calc(var(--k-space-4) + 12px + 76px + 3 * var(--k-space-2));
  }

  .compact .detail {
    flex-wrap: nowrap;
    padding-block: 0;
  }

  .compact .msg {
    flex: 1;
  }

  .msg {
    flex: 1 0 100%;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  /* The selected row's actions (#397): outlined like the Tickets row actions, the next step in accent. */
  .act {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    flex: none;
    padding: 0 var(--k-space-2) 0 var(--k-space-1);
    border: 1px solid var(--k-border-strong);
    border-radius: var(--k-radius-sm);
    background: none;
    font: inherit;
    font-size: var(--k-font-size-xs);
    line-height: 20px;
    color: var(--k-fg);
    cursor: pointer;
  }

  .act:hover {
    background: var(--k-bg-active);
  }

  .act.primary {
    border-color: var(--k-accent);
  }

  /* Next up drag (#397): the row it lands on carries the accent line. */
  .row.over {
    box-shadow: inset 0 2px 0 var(--k-accent);
  }

  .moreline {
    padding-left: calc(var(--k-space-4) + 12px + 2 * var(--k-space-2));
    color: var(--k-accent);
    cursor: pointer;
  }
</style>
