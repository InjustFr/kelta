<script lang="ts">
  // The Queue (FLOW §3): one screen across projects answering "review Claude's work, or grab a feature?".
  // It keeps the Inbox slot (rail top tile, ⌘0, `inbox.open`, pane kind `inbox`).
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError, ClaudeTask, SessionInfo, TicketRef } from '$lib/gen';
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
    LoadBar,
    Menu,
    ROW_HEIGHT,
    Spinner,
    VirtualList,
    relativeTime,
  } from '$lib/ui';

  import { chordFor } from '../../shell/labels';
  import { openView } from '../../shell/nav';
  import { answer, peekHover } from '../../shell/peek.svelte';
  import { listScope } from '../../shell/scope.svelte';
  import { ctxHot, itemCost, overBudget, usd } from '../../shell/usage';
  import { blockedReason, runWorkAction, workAction, WORK_ACTIONS } from '../work/actions';
  import { batch, batchKey } from '../work/batch.svelte';
  import { isAuthError } from '../work/common';
  import { sendToClaude } from '../work/fixloop.svelte';
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
    type NowSection,
    type WorkActionId,
  } from '../work/phase';
  import ActionMenu, { under, type RowAction } from '../work/shared/ActionMenu.svelte';
  import Brief, { type BriefAction, type Fact } from '../work/shared/Brief.svelte';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StageTrack from '../work/shared/StageTrack.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { onActions } from '../work/ui.svelte';
  import { CLAUDE, DEPLOY, PR, REVIEW, STAGES, TICKET, stage, workStage, type Stage } from '../work/stage';
  import RowButton from '../tickets/RowButton.svelte';
  import {
    isNextUp,
    scopeSections,
    ZONES,
    zoneCount,
    zoneOf,
    type NowRow,
    type Section,
    type Zone,
  } from './groups';
  import { asOf, enterRow, goToRow, nowSummary, refreshNow, reviewRowLocally, startNextUp } from './now';
  import { nextUp } from './nextUp.svelte';
  import { waitChip } from '../people/waiting';
  import TagChips from '../tags/TagChips.svelte';
  import { waitingChip } from '../reviews/nudge';
  import FilterBar from '../filters/FilterBar.svelte';
  import { clearAll, commonFilters, noMatch } from '../filters/filters';
  import { loopDone, loopShipped, rememberShipped } from '../onboarding/firstRun';
  import YourLoop from '../onboarding/YourLoop.svelte';

  let { focused }: PaneProps<'inbox'> = $props();

  const ALL = { kind: 'all' } as const;

  $effect(() => {
    untrack(() => void refreshNow());
  });
  // once at mount, not in an $effect: load() writes the state value() reads (a loop while pending)
  if (settings.value() === null) void settings.load().catch(() => undefined);

  /** No ticket source at all: Now says so and offers the source picker. */
  const noSource = $derived(settings.hasSources() === false);
  const pickBoard = () => ui.openSheet('tracker.source_picker', {});
  /** First run (#422): the loop's steps from real state, until a first item has shipped. */
  const loop = $derived(loopDone(projects.list, settings.hasSources(), work.all));
  // Only while setup is incomplete: with a project and a source the plain empty state is right.
  const showLoop = $derived(!(loop.project && loop.source) && !loop.ship && !loopShipped());
  $effect(() => {
    if (loop.ship) rememberShipped();
  });

  function refresh(): void {
    void refreshNow(true);
  }

  let filter = $state('');
  let filtering = $state(false);
  let filterEl = $state<HTMLInputElement>();

  const summary = $derived(nowSummary());
  /** The sections in the lists' scope (#409). */
  const sections = $derived(scopeSections(summary.sections, listScope.project));
  const yours = $derived(zoneCount(sections, 'yours'));
  const claude = $derived(zoneCount(sections, 'claude'));
  const others = $derived(zoneCount(sections, 'waiting'));
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
    id: string;
    title: string;
    project: string | null;
    color: string | null;
    /** The row's state ("Changes requested", "Claude working"): the brief's first line, and the row's
     *  next-step cell when it has no next action. */
    reason: string;
    /** The row's line 2: the one detail that says why (Claude's words, failed checks, who asked). */
    hint: string;
    /** Where it is on the way to deploy, and whose move (#399). */
    stage: Stage;
    /** The brief's facts: status and numbers. */
    facts: Fact[];
    /** Claude's last words, in the brief. */
    message: string;
    /** Work rows with a task list: how far Claude got (tasks, changes so far, top files). */
    progress?: { tasks: ClaudeTask[]; changes: string | null; files: string[] };
    /** Ticket and review rows: where their local tags live (#349). */
    tagged?: { kind: TagKind; key: string };
    /** Shown as a warning on the row (`tests: none` when source changed). */
    warn?: string;
    age: string;
    /** The brief's buttons, the key on each; the `enter` one is the primary and the row's next step. */
    actions: BriefAction[];
    /** A Claude session Kelta did not start: shown dimmed. */
    dim?: boolean;
  }

  function projectOf(ids: readonly string[]): { name: string | null; color: string | null } {
    const p = ids[0] ? projects.byId(ids[0]) : null;
    return { name: p?.name ?? null, color: p?.color ?? null };
  }

  const age = (iso: string): string => relativeTime(Date.parse(iso));
  const fact = (label: string, value: string | null | undefined, tone?: Fact['tone']): Fact[] =>
    value ? [{ label, value, ...(tone ? { tone } : {}) }] : [];

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
    C: 'start_claude',
  };

  function view(row: NowRow, section: NowSection): View {
    const yours = zoneOf(section) === 'yours';
    const mark = (broken = false) => (broken ? 'broken' : yours ? 'you' : 'wait');
    switch (row.type) {
      case 'work': {
        const { item, phase } = row;
        const git = work.git[item.id];
        const pr = prOf(item);
        const delta = phase.section === 'to_review' ? item.delta : null;
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
        const reason = row.status
          ? `${row.status} · ${phase.label}`
          : phase.section === 'needs_you' && phase.detail
            ? phase.detail
            : phase.label;
        // Waiting to deploy: its tracker status is the news; Claude asking: its question. A bare `#309`
        // (Merged, Reviewed) only repeats the id: the state says more.
        const hint = row.status || !more || /^#\d+$/.test(more) ? reason : more;
        const changes =
          git && (git.insertions || git.deletions)
            ? `+${git.insertions} −${git.deletions}${git.dirty ? ` ${t('uncommitted')}` : ''}`
            : null;
        return {
          id: workKey(item),
          title: workTitle(item),
          project: p.name,
          color: p.color,
          reason,
          hint,
          stage: workStage(item, phase, pr, yours, !!row.status),
          facts: [
            ...fact(t('Project'), p.name),
            ...fact(
              t('Changes'),
              delta ? deltaParts(delta).join(' · ') : changes,
              delta && testsMissing(delta) ? 'warn' : undefined,
            ),
            ...fact(t('Pull request'), pr ? `#${pr.ref.number}` : null),
            ...fact(
              t('Behind {base}', { base: item.base }),
              git?.behind ? tn('{n} commit', '{n} commits', git.behind) : null,
            ),
            ...fact(
              t('Context'),
              u && u.context_pct !== null ? `${Math.round(u.context_pct)}%` : null,
              u && ctxHot(u) ? 'warn' : undefined,
            ),
            ...fact(
              t('Cost'),
              spent > 0 ? usd(spent) : null,
              overBudget(spent, settings.value(item.project_id)?.claude.budget_usd) ? 'danger' : undefined,
            ),
            ...fact(t('Auto finish'), item.auto_finish ? t('armed') : null),
            ...fact(t('Parked'), phase.lamp === 'parked' && item.parked_at ? age(item.parked_at) : null),
            ...fact(
              t('Editor'),
              item.nvim_kept
                ? tn('nvim kept: {n} unsaved buffer', 'nvim kept: {n} unsaved buffers', item.nvim_kept)
                : null,
            ),
          ],
          message: item.claude_message?.trim() || preview,
          // shortcut: the top files are the first ones Claude touched, unranked; work_delta_files (#425) only covers reviewed..last, not the whole branch.
          progress: claude?.tasks.length
            ? {
                tasks: claude.tasks,
                changes: git?.files
                  ? `+${git.insertions} −${git.deletions}, ${tn('{n} file', '{n} files', git.files)}`
                  : null,
                files: claude.files_touched.slice(0, 3).map((f) => f.replace(`${item.worktree}/`, '')),
              }
            : undefined,
          warn: delta && testsMissing(delta) ? t('tests: none') : row.blocked ? t('Blocked') : undefined,
          // Ready for review: how long it has waited since Claude stopped.
          age: age(delta ? (item.claude_at ?? item.created_at) : item.created_at),
          actions,
        };
      }
      case 'session': {
        const s = row.session;
        const p = projectOf([s.project_id]);
        const preview = s.claude?.preview ?? '';
        return {
          id: s.kind.type,
          title: sessionLabel(s),
          project: p.name,
          color: p.color,
          reason: t('Needs input'),
          hint: preview || t('Needs input'),
          stage: stage(CLAUDE, mark()),
          facts: fact(t('Project'), p.name),
          message: preview,
          age: age(s.created_at),
          actions: [{ key: 'enter', label: t('Go to session'), run: () => enterRow(row) }],
        };
      }
      case 'outside': {
        const s = row.session;
        const ph = outsidePhase(s);
        return {
          id: s.ticket?.key ?? 'claude',
          title: s.title,
          project: null,
          color: null,
          reason: ph.label,
          hint: ph.label,
          stage: stage(CLAUDE, mark()),
          facts: [...fact(t('Branch'), s.branch), ...fact(t('Folder'), s.cwd)],
          message: s.preview ?? '',
          age: age(s.updated_at),
          actions: [],
          dim: true,
        };
      }
      case 'review': {
        const r = row.review.review;
        const p = projectOf(row.review.project_ids);
        // Who is waiting on whom: only while it sits with the reviewers, not once they sent it back to me.
        const chip =
          row.mine && row.reason === 'In review'
            ? waitingChip(row.review, settings.value()?.reviews.sla_hours ?? 24)
            : null;
        const checks = r.failed_check_names.length
          ? t('checks failed: {names}', { names: r.failed_check_names.join(', ') })
          : r.ci === 'failure'
            ? t('checks failed')
            : '';
        const red = row.reason === 'Checks failed';
        return {
          id: `#${r.ref.number}`,
          title: r.title,
          project: p.name ?? r.ref.repo,
          color: p.color,
          reason: t(row.reason),
          // A plain request reads as who asked; mine and "Updated since your review" read as the reason.
          // Red CI does not hide a pending review (#369): `Checks failed · review required`.
          hint:
            !row.mine && row.reason === 'Review requested'
              ? t('from {name}', { name: r.author.name })
              : red
                ? [checks || t(row.reason), reviewOf(r)].filter(Boolean).join(' · ')
                : (chip?.label ?? t(row.reason)),
          stage: red ? stage(PR, 'broken', r.decision ? REVIEW : PR) : stage(REVIEW, mark()),
          tagged: { kind: 'prs', key: reviewKey(r.ref) },
          facts: [
            ...fact(t('Project'), p.name ?? r.ref.repo),
            ...fact(
              t('Changes'),
              r.additions !== null && r.deletions !== null ? `+${r.additions} −${r.deletions}` : null,
            ),
            ...fact(t('Branch'), `${r.source_branch} ${t('into')} ${r.target_branch}`),
            ...fact(t('Checks'), checks, 'danger'),
          ],
          message: '',
          warn: chip?.late ? chip.label : undefined,
          age: age((!row.mine && r.requested_at) || r.updated_at),
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
          id: tk.ref.key,
          title: tk.title,
          project: p.name,
          color: p.color,
          reason: tk.status.name,
          hint: batch.has(tk.ref)
            ? t('marked')
            : (waitChip(row.ticket.waiting, { withWhy: true })?.label ?? tk.status.name),
          stage: section === 'waiting' ? stage(DEPLOY, 'wait') : stage(TICKET, mark()),
          tagged: { kind: 'tickets', key: ticketKey(tk.ref) },
          facts: [
            ...fact(t('Project'), p.name),
            ...fact(t('Status'), tk.status.name),
            ...fact(t('Priority'), tk.priority),
          ],
          message: '',
          warn: tk.blocked ? t('Blocked') : undefined,
          age: age(tk.updated_at),
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

  function matches(row: NowRow, section: NowSection, q: string): boolean {
    if (!q) return true;
    const v = view(row, section);
    const branch =
      row.type === 'work' ? row.item.branch : row.type === 'review' ? row.review.review.source_branch : '';
    return `${v.id} ${v.title} ${v.project ?? ''} ${branch}`.toLowerCase().includes(q.toLowerCase());
  }

  type Line =
    | { type: 'zone'; id: string; zone: Zone; label: string; hint: string; count: number }
    /** A folded section's header (Waiting to deploy). */
    | { type: 'fold'; id: string; section: Section; count: number; open: boolean }
    | { type: 'row'; id: string; row: NowRow; section: NowSection }
    /** The option buttons of the row's question (#356). */
    | { type: 'ask'; id: string; session: SessionInfo }
    | { type: 'more'; id: string; count: number };

  let selId = $state<string | null>(null);
  /** The brief shows every action (`.`); closes when the selection moves. */
  let allActions = $state(false);
  /** The brief's reply to Claude (#414); cleared when the selection moves. */
  let draft = $state('');
  /** Folded sections (Waiting to deploy) opened by their header. */
  let unfolded = $state<string[]>([]);
  const isOpen = (s: Section): boolean => !s.folded || unfolded.includes(s.id);
  const toggleSection = (s: Section): void => {
    unfolded = isOpen(s) ? unfolded.filter((id) => id !== s.id) : [...unfolded, s.id];
  };

  // Sections in zone order (#399, #413): Your move, Claude working, Waiting on others, each keeping its section order.
  const visible = $derived(
    ZONES.flatMap((z) => sections.filter((s) => zoneOf(s.id) === z.id))
      .map((s) => ({ ...s, rows: s.rows.filter((r) => matches(r, s.id, filter.trim())) }))
      .filter((s) => s.rows.length > 0 || s.more > 0),
  );
  const activeFilters = $derived(commonFilters(filter, () => (filter = '')));
  const selectable = $derived(visible.filter(isOpen).flatMap((s) => s.rows.map((row) => ({ row, s: s.id }))));
  const curAt = $derived(selectable.find((r) => r.row.id === selId) ?? selectable[0] ?? null);
  const cur = $derived(curAt?.row ?? null);
  const curView = $derived(curAt ? view(curAt.row, curAt.s) : null);

  // A primitive: a store update that rebuilds the same row must not clear a draft being typed.
  const curId = $derived(cur?.id);
  $effect(() => {
    void curId;
    allActions = false;
    draft = '';
  });

  const lines = $derived.by((): Line[] => {
    const out: Line[] = [];
    for (const z of ZONES) {
      const secs = visible.filter((s) => zoneOf(s.id) === z.id);
      if (secs.length === 0) continue;
      const count = secs.reduce((n, s) => n + s.rows.length + s.more, 0);
      out.push({ type: 'zone', id: `z:${z.id}`, zone: z.id, label: z.label, hint: z.hint, count });
      for (const s of secs) {
        const open = isOpen(s);
        if (s.folded)
          out.push({ type: 'fold', id: `s:${s.id}`, section: s, count: s.rows.length + s.more, open });
        if (!open) continue;
        for (const row of s.rows) {
          out.push({ type: 'row', id: row.id, row, section: s.id });
          const asker = rowAsker(row);
          if (asker) out.push({ type: 'ask', id: `a:${row.id}`, session: asker });
        }
        if (s.more > 0) out.push({ type: 'more', id: `m:${s.id}`, count: s.more });
      }
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

  function step(delta: number): void {
    if (selectable.length === 0) return;
    const at = curAt ? selectable.indexOf(curAt) : -1;
    const next = selectable[Math.min(selectable.length - 1, Math.max(0, at + delta))];
    if (next) selId = next.row.id;
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

  /** Next up as shown (in scope, unfiltered): `S` starts its top ones, `J`/`K` reorder within it. */
  const nextUpRows = (): NowRow[] => sections.find((s) => s.id === 'next_up')?.rows ?? [];

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
    const from = nextUpRows().find((r) => r.id === dragId);
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

  /** The full key list (`?`). */
  let keysOpen = $state(false);
  const hints = $derived<[string, string][]>([
    ['j k', msg('Move')],
    ['enter', msg('Next action')],
    ['.', msg('More actions')],
    ['g', msg('Go to')],
    ['/', msg('Filter')],
    ['space', msg('Mark ticket')],
    ['o', msg('Open in browser')],
    ['shift+r', msg('Refresh')],
    [chordFor('work.start') ?? 'mod+enter', msg('Start marked')],
  ]);

  function onkeydown(e: KeyboardEvent): void {
    // The filter and the brief's reply field keep their keys.
    if (e.target instanceof HTMLInputElement) return;
    // A section header keeps its own keys (Enter, Space); they must not act on the selected row.
    if (e.target instanceof HTMLButtonElement && e.target.classList.contains('fold')) return;
    // A brief button pressed with the keyboard runs itself, not the row's next action too. (The row itself is a button: don't match it.)
    if (e.target instanceof Element && e.target.closest('.side') && (e.key === 'Enter' || e.key === ' '))
      return;
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
        if (selectable[0]) selId = selectable[0].row.id;
        break;
      case 'End':
        if (selectable.at(-1)) selId = selectable.at(-1)!.row.id;
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
      case '.':
        allActions = !allActions;
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
      default: {
        // `1`–`9` answer the brief's question (#414).
        const asker = cur && /^[1-9]$/.test(e.key) ? answerable(cur) : null;
        const option = asker && questionOf(asker)?.options[Number(e.key) - 1];
        if (asker && option) {
          // One answer at a time: a held or repeated key must not type the option again.
          if (!inflight.has(cur.id)) void inflight.track(cur.id, () => answer(asker.id, option));
          break;
        }
        if (!cur || !'vVbpfrcansoPC'.includes(e.key) || e.key.length !== 1) return;
        letter(cur, e.key);
      }
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

  /** The question the brief answers: not behind a permission prompt (Kelta never types into one). */
  function answerable(row: NowRow): SessionInfo | null {
    const s = rowAsker(row);
    return s && s.status !== 'needs_input' ? s : null;
  }

  /** The reply field: types into Claude's question, or sends to a Claude that replied by `work_send`'s
   *  rules (pasted when idle, held until its Stop, or copied; core toasts the last two). */
  async function sendDraft(row: NowRow): Promise<void> {
    const text = draft.trim();
    const asker = answerable(row);
    if (!text) return;
    // Emptied first so a second Enter cannot send it twice; put back if it failed.
    draft = '';
    const sent = asker
      ? await answer(asker.id, text)
      : row.type === 'work' && (await sendToClaude(row.item, text));
    if (!sent && !draft) draft = text;
  }

  function onReplyKey(e: KeyboardEvent, row: NowRow): void {
    if (e.key === 'Escape') root?.focus();
    else if (e.key === 'Enter' && !e.isComposing) void sendDraft(row);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  // `⌘.` (#404): the brief's actions in a menu under the selected row's next step, that step first.
  let more = $state.raw<{ x: number; y: number; actions: RowAction[] } | null>(null);

  function menuKey(): boolean {
    const row = cur;
    const actions = curView?.actions ?? [];
    if (!row || actions.length === 0) return false;
    more = {
      ...under(root?.querySelector('.row.selected .next')),
      actions: actions.map((a) => ({
        id: a.key,
        label: a.label,
        key: a.key,
        run: () => inflight.track(row.id, a.run),
      })),
    };
    return true;
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

{#snippet count(text: string, n: number)}
  {@const at = text.indexOf(String(n))}
  {#if at < 0}{text}{:else}{text.slice(0, at)}<b class="k-num">{n}</b>{text.slice(at + String(n).length)}{/if}
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane"
  data-testid="inbox-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={t('Queue')}
  {onkeydown}
  use:onActions={menuKey}
>
  <header class="bar">
    <span class="title">{t('Queue')}</span>
    <!-- One summary line (#399): the zones below carry the rest. -->
    <span class="split" data-testid="now-header">
      <span>{@render count(t('{n} your move', { n: yours }), yours)}</span>
      <span>{@render count(t('{n} Claude working', { n: claude }), claude)}</span>
      <span>{@render count(t('{n} waiting on others', { n: others }), others)}</span>
    </span>
    {#if stale}<span class="asof" data-testid="now-asof"
        >{t('Updated {ago}', { ago: relativeTime(stale.ms) })}</span
      >{/if}
    <span class="spacer"></span>
    <IconButton
      icon="plus"
      size="sm"
      label={t('New work item')}
      chord={chordFor('work.new')}
      onclick={() => void dispatch('work.new')}
    />
  </header>
  <FilterBar
    bind:value={filter}
    bind:input={filterEl}
    active={activeFilters}
    placeholder={t('Filter id, title, project, branch (/)')}
    aria-label={t('Filter Queue')}
    onfocus={() => (filtering = true)}
    onkeydown={onFilterKey}
    onblur={() => (filtering = false)}
  />
  <LoadBar active={!loading && lists.some((l) => l.loading)} label={t('Loading Queue')} />

  {#if loading}
    <Loading label={t('Loading Queue')} />
  {:else if noData && firstError}
    <ErrorState error={firstError} title={t('Could not load the Queue')} onretry={refresh}>
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
    {#if lines.length === 0 && showLoop && activeFilters.length === 0}
      <YourLoop done={loop} />
    {:else if lines.length === 0 && noSource && activeFilters.length === 0}
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
      <div class="body">
        <div class="main">
          <div class="cols grid" aria-hidden="true">
            <span></span><span></span>
            <span class="ends"><span>{t(STAGES[0])}</span><span>{t(STAGES[STAGES.length - 1])}</span></span>
            <span class="next-h">{t('Next step')}</span><span class="right">{t('Last change')}</span>
          </div>
          <div class="list">
            <VirtualList
              bind:this={vlist}
              items={lines}
              itemHeight={ROW_HEIGHT}
              measure
              key={(l) => l.id}
              label={t('Queue')}
              sticky={(l) => l.type === 'zone'}
            >
              {#snippet row(l)}
                {#if l.type === 'zone'}
                  <div class="zone {l.zone}" role="heading" aria-level="2" data-zone={l.zone}>
                    {t(l.label)} <span class="count">{l.count}</span>
                    <span class="k-group-hint">{t(l.hint)}</span>
                  </div>
                {:else if l.type === 'fold'}
                  {@const s = l.section}
                  <div class="foldline" data-section={s.id}>
                    <button
                      type="button"
                      class="fold"
                      aria-expanded={l.open}
                      onclick={() => toggleSection(s)}
                    >
                      <span class="chevron"><Icon name="chevron-right" size={12} /></span>
                      {t(s.label)}
                      <span class="count">{l.count}</span>
                    </button>
                  </div>
                {:else if l.type === 'ask'}
                  <div class="askline"><AskBar session={l.session} /></div>
                {:else if l.type === 'more'}
                  <button type="button" tabindex="-1" class="moreline" onclick={() => void showAll()}>
                    {t('{n} more, show all', { n: l.count })}
                  </button>
                {:else}
                  {@const v = view(l.row, l.section)}
                  {@const next = v.actions.find((a) => a.key === 'enter')}
                  {@const sel = cur === l.row}
                  <button
                    type="button"
                    tabindex="-1"
                    class="row grid {zoneOf(l.section)}"
                    class:dim={v.dim}
                    class:selected={sel}
                    class:over={overId === l.row.id && dragId !== l.row.id}
                    aria-current={sel ? 'true' : undefined}
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
                    <span class="id">{v.id}</span>
                    <span class="what">
                      <span class="ttl">{v.title}</span>
                      <span class="why">
                        {#if v.hint}<span class="hint" class:warn={v.hint === v.warn} title={v.hint}
                            >{v.hint}</span
                          >{/if}
                        {#if v.warn && v.warn !== v.hint}<span class="meta warn">{v.warn}</span>{/if}
                        {#if v.tagged}<TagChips kind={v.tagged.kind} itemKey={v.tagged.key} />{/if}
                      </span>
                    </span>
                    <span class="stage"><StageTrack stage={v.stage} named={sel} /></span>
                    <!-- The next step is a real control (#399): it runs, it is not a styled label. -->
                    <span class="next">
                      {#if next}<RowButton
                          label={next.label}
                          onclick={() => void inflight.track(l.row.id, next.run)}
                          ><span class="act" class:broken={v.stage.mark === 'broken'} data-next
                            >{#if inflight.has(l.row.id)}<Spinner size={10} />{/if}{next.label}</span
                          ></RowButton
                        >{:else}<span class="reason"
                          >{#if inflight.has(l.row.id)}<Spinner size={10} />{/if}{v.reason}</span
                        >{/if}
                    </span>
                    <span class="age">{v.age}</span>
                  </button>
                {/if}
              {/snippet}
            </VirtualList>
          </div>
        </div>
        {#if cur && curView}
          {@const row = cur}
          {@const v = curView}
          {@const track = (a: BriefAction): BriefAction => ({
            ...a,
            run: () => inflight.track(row.id, a.run),
          })}
          {@const asker = answerable(row)}
          {@const replied = row.type === 'work' && row.phase.id === 'replied'}
          <div class="side">
            <Brief
              id={v.id}
              title={v.title}
              where={asker ? '' : v.reason}
              primary={v.actions.filter((a) => a.key === 'enter').map(track)[0] ?? null}
              actions={v.actions.filter((a) => a.key !== 'enter').map(track)}
              bind:more={allActions}
              message={v.message}
              facts={v.facts}
              busy={inflight.has(row.id)}
              testid="now-brief"
            >
              {#snippet sub()}{#if asker}<AskBar session={asker} full numbered />{/if}{/snippet}
              {#snippet reply()}
                {#if asker || replied}
                  <input
                    bind:value={draft}
                    class="k-filter reply"
                    placeholder={t('Reply to Claude')}
                    aria-label={t('Reply to Claude')}
                    data-testid="now-reply"
                    onkeydown={(e) => onReplyKey(e, row)}
                  />
                {/if}
              {/snippet}
              {#if v.progress}
                {@const { tasks, changes, files } = v.progress}
                <section class="progress" data-testid="now-brief-progress">
                  <h4>
                    {t('Tasks')}
                    <span
                      >{t('{done} of {n}', {
                        done: tasks.filter((k) => k.state === 'completed').length,
                        n: tasks.length,
                      })}</span
                    >
                  </h4>
                  <ul class="tasks">
                    {#each tasks as task, i (task.id ?? i)}
                      <li
                        data-state={task.state}
                        aria-current={task.state === 'in_progress' ? 'step' : undefined}
                      >
                        <span aria-hidden="true"
                          >{task.state === 'completed' ? '✓' : task.state === 'in_progress' ? '▸' : '○'}</span
                        >{task.title}
                      </li>
                    {/each}
                  </ul>
                  {#if changes}
                    <h4>{t('Changes so far')} <span>{changes}</span></h4>
                    <ul class="files">
                      {#each files as f (f)}<li>{f}</li>{/each}
                    </ul>
                  {/if}
                </section>
              {/if}
            </Brief>
          </div>
        {/if}
      </div>
    {/if}
  {/if}
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
{#if more}
  <ActionMenu
    {...more}
    lit
    onclose={() => {
      more = null;
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
    container: now / inline-size;
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
    padding: 0 var(--k-space-3) 0 var(--k-space-4);
    border-bottom: 1px solid var(--k-border);
    background: var(--k-bg-elev);
  }

  .title {
    flex: none;
    font-weight: 600;
  }

  .split {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 0 var(--k-space-4);
    min-width: 0;
    font-size: var(--k-font-size-sm);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-muted);
  }

  .split :global(b) {
    font-weight: 600;
    color: var(--k-fg);
  }

  .asof {
    font-size: var(--k-font-size-xs);
    color: var(--k-warn);
  }

  .spacer {
    flex: 1;
  }

  /* The list, and the selected item's brief beside it (#399): 340px, 300px below 1180px, under the
     list below 900px. */
  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 340px;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .list {
    flex: 1;
    min-height: 0;
  }

  .side {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    border-left: 1px solid var(--k-border);
    background: var(--k-bg-elev);
  }

  /* id, title and why, the stage track, the next step, the age: one grid for the header and the rows. */
  .grid {
    display: grid;
    grid-template-columns: 76px minmax(0, 1fr) 160px 180px 64px;
    column-gap: var(--k-space-4);
    align-items: center;
    padding-inline: var(--k-space-4);
  }

  .cols {
    flex: none;
    padding-block: var(--k-space-2) var(--k-space-1);
    border-bottom: 1px solid var(--k-border);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .ends {
    display: flex;
    justify-content: space-between;
  }

  .right {
    text-align: right;
  }

  .zone {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-2);
    height: 100%;
    padding: var(--k-space-4) var(--k-space-4) var(--k-space-1);
    background: var(--k-bg);
    font-size: var(--k-font-size-sm);
    font-weight: 600;
    color: var(--k-fg-strong);
  }

  .zone.waiting {
    color: var(--k-fg-muted);
  }

  .count {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
  }

  .foldline {
    padding: var(--k-space-1) var(--k-space-4);
  }

  .fold {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    padding: var(--k-space-1) 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
    cursor: pointer;
  }

  /* The chevron turns as the section folds or opens; the rows fade in and out (VirtualList). */
  .chevron {
    display: inline-flex;
  }

  .chevron :global(svg) {
    transition: rotate var(--k-duration-fast) var(--k-ease);
  }

  .fold[aria-expanded='true'] .chevron :global(svg) {
    rotate: 90deg;
  }

  .row {
    width: 100%;
    min-height: 56px;
    padding-block: var(--k-space-2);
    border: 0;
    box-shadow: inset 3px 0 0 var(--hue);
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--k-font-size);
    text-align: left;
    cursor: default;
    --track-bg: var(--k-bg);
  }

  .row:hover {
    background: var(--k-bg-hover);
  }

  /* Outside Kelta (#345): the words quieter. */
  .row.dim > :not(.stage) {
    opacity: 0.6;
  }

  /* Subtle and muted text on the selected row take their -sel steps (#399: 3.58:1 before, in light). */
  .selected {
    background: var(--k-bg-selected-row);
    --k-fg-subtle: var(--k-fg-subtle-sel);
    --k-fg-muted: var(--k-fg-muted-sel);
    --track-bg: var(--k-bg-selected-row);
  }

  .id {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  /* Titles wrap to two lines instead of one cut line (#399). */
  .ttl {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    color: var(--k-fg-strong);
  }

  .waiting .ttl {
    color: var(--k-fg-muted);
  }

  .why {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .hint {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-subtle);
  }

  .hint.warn,
  .meta.warn {
    color: var(--k-warn);
  }

  .meta {
    flex: none;
    font-size: var(--k-font-size-xs);
  }

  .next {
    display: flex;
    min-width: 0;
  }

  .next :global(.rb) {
    max-width: 100%;
  }

  /* The next step: outlined, accent when it is my move, danger when something broke. */
  .act {
    max-width: 100%;
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    padding: 3px var(--k-space-2);
    border: 1px solid var(--k-border-strong);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .yours .act {
    border-color: var(--k-accent);
    color: var(--k-accent);
    font-weight: var(--k-weight-medium);
  }

  .yours .act.broken {
    border-color: var(--k-danger);
    color: var(--k-danger);
  }

  .act:hover {
    background: var(--k-bg-active);
  }

  .reason {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .age {
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    text-align: right;
    color: var(--k-fg-subtle);
  }

  .nosource {
    padding: var(--k-space-2) var(--k-space-4);
    border-bottom: 1px solid var(--k-border);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .askline {
    padding-left: calc(var(--k-space-4) + 76px + var(--k-space-4));
  }

  /* Next up drag (#397): the row it lands on carries the accent line. */
  .row.over {
    box-shadow: inset 0 2px 0 var(--k-accent);
  }

  .moreline {
    width: 100%;
    padding: var(--k-space-2) var(--k-space-4) var(--k-space-2)
      calc(var(--k-space-4) + 76px + var(--k-space-4));
    border: 0;
    background: none;
    font: inherit;
    font-size: var(--k-font-size-sm);
    text-align: left;
    color: var(--k-accent);
    cursor: pointer;
  }

  @container now (max-width: 1115px) {
    .body {
      grid-template-columns: minmax(0, 1fr) 300px;
    }

    .grid {
      grid-template-columns: 76px minmax(0, 1fr) 130px 64px;
      row-gap: var(--k-space-1);
    }

    .next-h {
      display: none;
    }

    .row .next {
      grid-column: 2;
      grid-row: 2;
    }

    .row .stage {
      grid-column: 3;
      grid-row: 1;
    }

    .row .age {
      grid-column: 4;
      grid-row: 1;
    }
  }

  /* Narrow: the brief stacks under the list, which keeps at least half the height. Pane widths: the window minus the 64px compact rail (835 = a 900px window, 1115 = 1180px). */
  @container now (max-width: 835px) {
    .body {
      display: flex;
      flex-direction: column;
    }

    .main {
      flex: 1 1 0;
    }

    .side {
      flex: 0 1 auto;
      max-height: 50%;
      border-left: 0;
      border-top: 1px solid var(--k-border);
    }
  }

  @container now (max-width: 699px) {
    .cols {
      display: none;
    }

    .grid {
      grid-template-columns: 64px minmax(0, 1fr) 56px;
      padding-inline: var(--k-space-3);
    }

    .row .stage {
      grid-column: 2 / 4;
      grid-row: 2;
    }

    .row .next {
      grid-column: 2 / 4;
      grid-row: 3;
    }

    .row .age {
      grid-column: 3;
      grid-row: 1;
    }

    .askline,
    .moreline {
      padding-left: calc(var(--k-space-3) + 64px + var(--k-space-4));
    }
  }

  /* The brief's reply field (#414). */
  .reply {
    width: 100%;
  }

  /* Claude's progress in the brief (#434). */
  .progress h4 {
    display: flex;
    justify-content: space-between;
    gap: var(--k-space-3);
    margin: 0 0 var(--k-space-1);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg-muted);
  }

  .progress h4 span {
    font-weight: normal;
    font-variant-numeric: tabular-nums;
  }

  .progress ul {
    margin: 0 0 var(--k-space-3);
    padding: 0;
    list-style: none;
    overflow-wrap: anywhere;
  }

  .tasks li {
    display: flex;
    gap: var(--k-space-2);
  }

  .tasks li > span {
    flex: none;
    width: 1ch;
    color: var(--k-fg-muted);
  }

  .tasks li[data-state='completed'] {
    color: var(--k-fg-muted);
  }

  .tasks li[data-state='in_progress'] {
    color: var(--k-fg-strong);
    font-weight: var(--k-weight-strong);
  }

  .tasks li[data-state='in_progress'] > span {
    color: var(--k-accent);
  }

  .files li {
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
  }
</style>
