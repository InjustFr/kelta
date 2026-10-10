<script lang="ts">
  // Now (FLOW §3): one screen across projects answering "review Claude's work, or grab a feature?".
  // It keeps the Inbox slot (rail top tile, ⌘0, `inbox.open`, pane kind `inbox`).
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError } from '$lib/gen';
  import { openExternal } from '$lib/ipc/commands';
  import { projects, reviews, tickets, toasts, work } from '$lib/stores';
  import { Button, EmptyState, ErrorState, Kbd, Lamp, ROW_HEIGHT, VirtualList, relativeTime } from '$lib/ui';

  import { blockedReason, runWorkAction, WORK_ACTIONS } from '../work/actions';
  import { isAuthError } from '../work/common';
  import { claudeOf, prOf, sessionLabel, workTitle } from '../work/live';
  import { openFromNow } from '../work/nav';
  import { deltaParts, testsMissing, workKey, type Lamp as LampKind, type WorkActionId } from '../work/phase';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import type { NowRow, Section } from './groups';
  import { asOf, enterRow, goToRow, nowSummary, refreshNow, reviewRowLocally } from './now';

  let { focused }: PaneProps<'inbox'> = $props();

  const ALL = { kind: 'all' } as const;

  $effect(() => {
    untrack(() => void refreshNow());
  });

  function refresh(): void {
    void refreshNow(true);
  }

  let filter = $state('');
  let filtering = $state(false);
  let filterEl = $state<HTMLInputElement>();

  const summary = $derived(nowSummary());
  const stale = $derived(asOf());
  const lists = $derived([
    tickets.list(ALL, null),
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
    /** The meta part shown as a warning (`tests: none` when source changed). */
    warn?: string;
    age: string;
    /** Second line of the selected row. */
    more: string;
    /** Ghost buttons: key + label. */
    actions: { key: string; label: string }[];
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
  };

  function view(row: NowRow): View {
    switch (row.type) {
      case 'work': {
        const { item, phase } = row;
        const git = work.git[item.id];
        const pr = prOf(item);
        const meta: string[] = [];
        const delta = phase.section === 'to_review' ? item.delta : null;
        if (delta) meta.push(...deltaParts(delta));
        else if (git && (git.insertions || git.deletions))
          meta.push(`+${git.insertions} −${git.deletions}${git.dirty ? ' uncommitted' : ''}`);
        else if (pr) meta.push(`#${pr.ref.number}`);
        if (git?.behind) meta.push(`${git.behind} behind ${item.base}`);
        if (item.auto_finish) meta.push('armed');
        const p = projectOf([item.project_id]);
        const actions = [
          ...(phase.primary ? [{ key: 'enter', label: phase.primaryLabel }] : []),
          ...WORK_ACTIONS.filter(
            (a) =>
              a.key && Object.values(NOW_KEYS).includes(a.id) && blockedReason(a.id, item, phase) === null,
          ).map((a) => ({ key: a.key!, label: a.label({ item, phase }) })),
          ...(item.claude_message && phase.section === 'to_review'
            ? [{ key: 'm', label: expanded === row.id ? 'Hide message' : 'Full message' }]
            : []),
          { key: 'g', label: 'Go to work tab' },
        ];
        const preview = claudeOf(item)?.claude?.preview ?? '';
        const said = item.claude_message?.trim().split('\n')[0] || preview;
        const more =
          phase.section === 'to_review'
            ? [item.next_note ? `next: ${item.next_note}` : '', said].filter(Boolean).join('  ·  ') ||
              phase.detail
            : phase.section === 'needs_you' && preview
              ? preview
              : phase.detail;
        return {
          lamp: phase.lamp,
          id: workKey(item),
          title: workTitle(item),
          project: p.name,
          color: p.color,
          reason: phase.section === 'needs_you' && phase.detail ? phase.detail : phase.label,
          meta,
          warn: delta && testsMissing(delta) ? 'tests: none' : undefined,
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
          reason: preview || 'Needs input',
          meta: [],
          age: age(s.created_at),
          more: preview,
          actions: [{ key: 'enter', label: 'Go to session' }],
        };
      }
      case 'review': {
        const r = row.review.review;
        const p = projectOf(row.review.project_ids);
        const stat = r.additions !== null && r.deletions !== null ? [`+${r.additions} −${r.deletions}`] : [];
        return {
          lamp: 'none',
          id: `#${r.ref.number}`,
          title: r.title,
          project: p.name ?? r.ref.repo,
          color: p.color,
          // A plain request reads as who asked; mine and "Updated since your review" read as the reason.
          reason: !row.mine && row.reason === 'Review requested' ? r.author.name : row.reason,
          meta: stat,
          age: age(r.updated_at),
          more: `${r.source_branch} into ${r.target_branch}${r.ci === 'failure' ? ', checks failed' : ''}`,
          actions: [
            { key: 'enter', label: 'Open review' },
            ...(row.mine ? [] : [{ key: 's', label: 'Review locally' }]),
            { key: 'o', label: 'Open on host' },
          ],
        };
      }
      case 'ticket': {
        const t = row.ticket.ticket;
        const p = projectOf(row.ticket.project_ids);
        return {
          lamp: 'none',
          id: t.ref.key,
          title: t.title,
          project: p.name,
          color: p.color,
          reason: t.status.name,
          meta: t.priority ? [t.priority] : [],
          age: age(t.updated_at),
          more: t.assignee?.name ?? '',
          actions: [
            { key: 'enter', label: 'Start work' },
            { key: 'o', label: 'Open in tracker' },
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
    | { type: 'section'; id: string; section: Section; count: number }
    | { type: 'row'; id: string; row: NowRow }
    | { type: 'detail'; id: string; row: NowRow }
    | { type: 'msg'; id: string; text: string }
    | { type: 'more'; id: string; count: number };

  let selId = $state<string | null>(null);
  /** Row whose full Claude message is unfolded under its detail line (`m`). */
  let expanded = $state<string | null>(null);

  const visible = $derived(
    summary.sections
      .map((s) => ({ ...s, rows: s.rows.filter((r) => matches(r, filter)) }))
      .filter((s) => s.rows.length > 0 || s.more > 0),
  );
  const selectable = $derived(visible.flatMap((s) => s.rows));
  const cur = $derived(selectable.find((r) => r.id === selId) ?? selectable[0] ?? null);

  const lines = $derived.by((): Line[] => {
    const out: Line[] = [];
    for (const s of visible) {
      out.push({ type: 'section', id: `s:${s.id}`, section: s, count: s.rows.length + s.more });
      for (const row of s.rows) {
        out.push({ type: 'row', id: row.id, row });
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
    if (url) openExternal({ url }).catch((err) => toasts.error(err, 'Open in browser'));
    else if (row.type === 'work') void runWorkAction('open_ticket', row.item);
  }

  function letter(row: NowRow, key: string): void {
    if (key === 'o') return browse(row);
    if (row.type === 'work') {
      const id = NOW_KEYS[key];
      if (id) void runWorkAction(id, row.item);
      return;
    }
    if (key === 's' && row.type === 'ticket') void enterRow(row);
    else if (key === 's' && row.type === 'review' && !row.mine) reviewRowLocally(row);
  }

  function showAllOnBoard(): void {
    const pid = projects.activeId ?? projects.list.find((p) => p.open && !p.builtin)?.id;
    if (pid) void openFromNow(pid, { kind: 'tickets', scope: ALL, view_id: null, mode: 'board' });
  }

  function onkeydown(e: KeyboardEvent): void {
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
        if (cur) void enterRow(cur);
        break;
      case 'g':
        if (cur) void goToRow(cur);
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
        toasts.info('New work item: not available yet');
        break;
      default:
        if (!cur || !'vVbpfrcanso'.includes(e.key) || e.key.length !== 1) return;
        letter(cur, e.key);
    }
    e.preventDefault();
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
  data-testid="inbox-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Now"
  {onkeydown}
>
  <header class="bar">
    <span class="heading">
      <span class="title">Now</span>
      <span class="subtitle">Everything waiting on you, across projects</span>
    </span>
    <span class="split" data-testid="now-header" title={summary.header}>
      {#each summary.parts as part (part)}<span>{part}</span>{/each}
    </span>
    {#if stale}<span class="asof" data-testid="now-asof">Updated {relativeTime(stale.ms)}</span>{/if}
    <span class="spacer"></span>
    <input
      bind:this={filterEl}
      bind:value={filter}
      class="k-filter"
      placeholder="Filter"
      aria-label="Filter Now"
      onfocus={() => (filtering = true)}
      onkeydown={onFilterKey}
      onblur={() => (filtering = false)}
    />
    <Button variant="ghost" size="sm" icon="refresh-cw" chord="shift+r" onclick={refresh}>Refresh</Button>
  </header>

  {#if loading}
    <Loading label="Loading Now" />
  {:else if noData && firstError}
    <ErrorState error={firstError} title="Could not load Now" onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
          {isAuthError(firstError) ? 'Re-authenticate' : 'Open account settings'}
        </Button>
      {/snippet}
    </ErrorState>
  {:else}
    <StateBanner error={noData ? null : firstError} errors={accountErrors} onretry={refresh} />
    {#if lines.length === 0}
      <EmptyState
        icon="inbox"
        title={filter ? `Nothing matches "${filter}".` : 'Nothing needs you right now'}
        body={filter
          ? ''
          : 'Claude sessions that ask a question, pull requests waiting for your review and tickets assigned to you show up here.'}
      >
        {#snippet actions()}
          {#if filter}<Button onclick={() => (filter = '')}>Clear filter</Button>{:else}<Button
              onclick={refresh}>Refresh</Button
            >{/if}
        {/snippet}
      </EmptyState>
    {:else}
      <div class="list">
        <VirtualList bind:this={vlist} items={lines} itemHeight={ROW_HEIGHT} key={(l) => l.id} label="Now">
          {#snippet row(l)}
            {#if l.type === 'section'}
              <div class="section" role="heading" aria-level="2" data-section={l.section.id}>
                <span class="slot"><Lamp level={l.section.lamp} title="" /></span>
                {l.section.label}
                <span class="count">{l.count}</span>
              </div>
            {:else if l.type === 'msg'}
              <div class="row msgline" data-testid="now-message">{l.text}</div>
            {:else if l.type === 'more'}
              <button type="button" tabindex="-1" class="row moreline" onclick={showAllOnBoard}>
                {l.count} more, show all on Board
              </button>
            {:else}
              {@const v = view(l.row)}
              {#if l.type === 'row'}
                <button
                  type="button"
                  tabindex="-1"
                  class="row"
                  class:selected={cur === l.row}
                  aria-current={cur === l.row ? 'true' : undefined}
                  data-row={l.row.id}
                  style:--hue={v.color ?? 'transparent'}
                  onclick={() => (selId = l.row.id)}
                  ondblclick={() => void enterRow(l.row)}
                >
                  <span class="slot"><Lamp level={v.lamp} /></span>
                  <span class="id">{v.id}</span>
                  <span class="ttl">{v.title}</span>
                  {#if v.project}<span class="proj">{v.project}</span>{/if}
                  <span class="reason">{v.reason}</span>
                  {#each v.meta as m (m)}<span class="meta" class:warn={m === v.warn}>{m}</span>{/each}
                  <span class="meta age">{v.age}</span>
                </button>
              {:else}
                <div class="row detail" style:--hue={v.color ?? 'transparent'} data-testid="now-detail">
                  <span class="msg" title={v.more}>{v.more}</span>
                  {#each v.actions as a (a.key)}
                    <span class="ghost"><Kbd chord={a.key} />{a.label}</span>
                  {/each}
                </div>
              {/if}
            {/if}
          {/snippet}
        </VirtualList>
      </div>
    {/if}
  {/if}

  <footer class="hints">
    <span><Kbd chord="j" /><Kbd chord="k" /> Move</span>
    <span><Kbd chord="enter" /> Next action</span>
    <span><Kbd chord="g" /> Go to</span>
    <span><Kbd chord="o" /> Open in browser</span>
    <span><Kbd chord="/" /> Filter</span>
    <span><Kbd chord="shift+r" /> Refresh</span>
  </footer>
</div>

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
    min-height: var(--k-tabbar-height);
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
    gap: var(--k-space-4);
    min-width: 0;
    overflow: hidden;
    font-size: var(--k-font-size-sm);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-muted);
    white-space: nowrap;
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
    height: 100%;
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

  .selected,
  .detail {
    background: var(--k-bg-selected);
  }

  .id {
    flex: none;
    width: 76px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .meta.warn {
    color: var(--k-warn);
  }

  .msgline {
    padding-left: calc(var(--k-space-4) + 12px + 76px + 3 * var(--k-space-2));
    background: var(--k-bg-selected);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .age {
    width: 64px;
    text-align: right;
  }

  .detail {
    gap: var(--k-space-3);
    padding-left: calc(var(--k-space-4) + 12px + 76px + 3 * var(--k-space-2));
  }

  .msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .ghost {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    flex: none;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .moreline {
    padding-left: calc(var(--k-space-4) + 12px + 2 * var(--k-space-2));
    color: var(--k-accent);
    cursor: pointer;
  }

  /* One line tall: hints that wrap fall out of view instead of adding a second line. */
  .hints {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0 var(--k-space-5);
    flex: none;
    height: var(--k-statusbar-height);
    overflow: hidden;
    padding: 0 var(--k-space-3);
    border-top: 1px solid var(--k-border);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
    white-space: nowrap;
  }

  .hints > span {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: var(--k-statusbar-height);
  }
</style>
