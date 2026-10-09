<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError, ReviewItem, SessionInfo, TicketItem } from '$lib/gen';
  import { openExternal } from '$lib/ipc/commands';
  import { activateTab, findSession, focusPane } from '$lib/layout';
  import { layout, projects, reviews, sessions, tickets, toasts } from '$lib/stores';
  import { reviewKey } from '$lib/stores/reducers';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Badge, Button, EmptyState, ErrorState, IconButton, VirtualList, relativeTime } from '$lib/ui';

  import { ciGlyph, decisionInfo, isAuthError, statusTone } from '../work/common';
  import { openContent } from '../work/nav';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { reviewLocally, startWorkOnTicket } from '../work/startWork';
  import { groupByProject } from './groups';

  let { projectId, focused }: PaneProps<'inbox'> = $props();

  const ROW_HEIGHT = 32;
  const ALL = { kind: 'all' } as const;

  const myTickets = $derived(tickets.list(ALL, null));
  const requested = $derived(reviews.list(ALL, 'review_requested'));
  const authored = $derived(reviews.list(ALL, 'authored'));

  $effect(() => {
    untrack(() => {
      void tickets.load(ALL, null);
      void reviews.load(ALL, 'review_requested');
      void reviews.load(ALL, 'authored');
    });
  });

  function refresh(): void {
    void tickets.load(ALL, null, true);
    void reviews.load(ALL, 'review_requested', true);
    void reviews.load(ALL, 'authored', true);
  }

  const order = $derived(
    projects.list.filter((p) => p.open && !p.builtin).map((p) => ({ id: p.id, name: p.name })),
  );
  const needing = $derived(sessions.needingInput);
  const noData = $derived(!myTickets.data && !requested.data && !authored.data);
  const loading = $derived((myTickets.loading || requested.loading || authored.loading) && noData);
  const firstError = $derived(myTickets.error ?? requested.error ?? authored.error);
  const stale = $derived(myTickets.stale || requested.stale || authored.stale);
  const fetchedAt = $derived(myTickets.fetchedAt ?? requested.fetchedAt ?? authored.fetchedAt);
  const accountErrors = $derived.by(() => {
    const seen: Record<string, AccountError> = {};
    for (const l of [myTickets, requested, authored])
      for (const e of l.data?.errors ?? []) seen[`${e.account_id}:${e.error.code}`] = e;
    return Object.values(seen);
  });

  type Row =
    | { type: 'section'; id: string; label: string; count: number }
    | { type: 'group'; id: string; label: string; count: number }
    | { type: 'empty'; id: string; label: string }
    | { type: 'ticket'; id: string; item: TicketItem }
    | { type: 'review'; id: string; item: ReviewItem; mine: boolean }
    | { type: 'session'; id: string; session: SessionInfo };

  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    const tItems = myTickets.data?.items ?? [];
    out.push({ type: 'section', id: 's:tickets', label: 'My tickets', count: tItems.length });
    if (tItems.length === 0) out.push({ type: 'empty', id: 'e:tickets', label: 'Nothing assigned to you.' });
    for (const g of groupByProject(tItems, (i) => i.project_ids, order)) {
      out.push({ type: 'group', id: `g:tickets:${g.id}`, label: g.name, count: g.items.length });
      for (const item of g.items) out.push({ type: 'ticket', id: `t:${ticketKey(item.ticket.ref)}`, item });
    }

    const rItems = requested.data?.items ?? [];
    out.push({ type: 'section', id: 's:requested', label: 'Review requests', count: rItems.length });
    if (rItems.length === 0) out.push({ type: 'empty', id: 'e:requested', label: 'No review requests.' });
    for (const g of groupByProject(rItems, (i) => i.project_ids, order)) {
      out.push({ type: 'group', id: `g:requested:${g.id}`, label: g.name, count: g.items.length });
      for (const item of g.items)
        out.push({ type: 'review', id: `r:${reviewKey(item.review.ref)}`, item, mine: false });
    }

    const aItems = authored.data?.items ?? [];
    out.push({ type: 'section', id: 's:authored', label: 'My PRs', count: aItems.length });
    if (aItems.length === 0) out.push({ type: 'empty', id: 'e:authored', label: 'No open pull requests.' });
    for (const g of groupByProject(aItems, (i) => i.project_ids, order)) {
      out.push({ type: 'group', id: `g:authored:${g.id}`, label: g.name, count: g.items.length });
      for (const item of g.items)
        out.push({ type: 'review', id: `p:${reviewKey(item.review.ref)}`, item, mine: true });
    }

    out.push({ type: 'section', id: 's:input', label: 'Needs input', count: needing.length });
    if (needing.length === 0) out.push({ type: 'empty', id: 'e:input', label: 'No session needs input.' });
    for (const session of needing) out.push({ type: 'session', id: `n:${session.id}`, session });
    return out;
  });

  const selectable = $derived(
    rows.filter((r) => r.type === 'ticket' || r.type === 'review' || r.type === 'session'),
  );
  let selId = $state<string | null>(null);
  const cur = $derived(selectable.find((r) => r.id === selId) ?? selectable[0] ?? null);
  let vlist = $state<{ scrollToIndex(i: number): void }>();
  let root = $state<HTMLDivElement>();

  $effect(() => {
    if (cur) vlist?.scrollToIndex(rows.indexOf(cur));
  });

  $effect(() => {
    if (focused && cur?.type === 'ticket')
      selectTicket(cur.item.ticket.ref, cur.item.project_ids[0] ?? projectId);
  });

  $effect(() => {
    if (focused && root && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
  });

  function step(delta: number): void {
    if (selectable.length === 0) return;
    const at = cur ? selectable.indexOf(cur) : -1;
    const next = selectable[Math.min(selectable.length - 1, Math.max(0, at + delta))];
    if (next) selId = next.id;
  }

  async function goToSession(s: SessionInfo): Promise<void> {
    try {
      await projects.activate(s.project_id);
      await layout.ensure(s.project_id);
      const l = layout.get(s.project_id);
      const loc = l ? findSession(l, s.id) : null;
      if (loc)
        layout.update(s.project_id, (x) => activateTab(focusPane(x, loc.tabId, loc.paneId), loc.tabId));
      else toasts.info(`${s.name} is not shown in a pane, find it in the palette`);
    } catch (err) {
      toasts.error(err, 'Open session');
    }
  }

  async function open(r: Row | null): Promise<void> {
    if (!r) return;
    if (r.type === 'session') {
      await goToSession(r.session);
      return;
    }
    if (r.type !== 'ticket' && r.type !== 'review') return;
    const pid = r.item.project_ids[0] ?? projects.activeId ?? projectId;
    try {
      if (pid !== projects.activeId) await projects.activate(pid);
    } catch (err) {
      toasts.error(err, 'Switch project');
      return;
    }
    if (r.type === 'ticket') await openContent(pid, { kind: 'ticket_detail', ticket: r.item.ticket.ref });
    else await openContent(pid, { kind: 'review_detail', review: r.item.review.ref });
  }

  function browse(r: Row | null): void {
    const url = r?.type === 'ticket' ? r.item.ticket.url : r?.type === 'review' ? r.item.review.url : null;
    if (url) openExternal({ url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  function start(r: Row | null): void {
    if (r?.type === 'ticket') void startWorkOnTicket(r.item.ticket.ref, r.item.project_ids[0] ?? null);
    else if (r?.type === 'review') void reviewLocally(r.item.review.ref, r.item.project_ids[0] ?? null);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
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
        void open(cur);
        break;
      case 'o':
        browse(cur);
        break;
      case 's':
        start(cur);
        break;
      case 'R':
        refresh();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  const empty = $derived(
    !loading &&
      (myTickets.data?.items.length ?? 0) === 0 &&
      (requested.data?.items.length ?? 0) === 0 &&
      (authored.data?.items.length ?? 0) === 0 &&
      needing.length === 0 &&
      !noData,
  );
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane"
  data-testid="inbox-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Inbox"
  {onkeydown}
>
  <header class="bar">
    <span class="title">Inbox</span>
    <span class="spacer"></span>
    <IconButton icon="refresh-cw" label="Refresh (R)" size="sm" onclick={refresh} />
  </header>

  {#if loading}
    <Loading label="Loading inbox" />
  {:else if noData && firstError}
    <ErrorState error={firstError} title="Could not load the inbox" onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
          {isAuthError(firstError) ? 'Re-authenticate' : 'Open settings'}
        </Button>
      {/snippet}
    </ErrorState>
  {:else}
    <StateBanner
      {stale}
      {fetchedAt}
      error={noData ? null : firstError}
      errors={accountErrors}
      onretry={refresh}
    />
    {#if empty && accountErrors.length === 0}
      <EmptyState icon="inbox" title="Inbox zero" body="No tickets, review requests or sessions need you.">
        {#snippet actions()}<Button onclick={refresh}>Refresh</Button>{/snippet}
      </EmptyState>
    {:else}
      <div class="list">
        <VirtualList
          bind:this={vlist}
          items={rows}
          itemHeight={ROW_HEIGHT}
          key={(r) => r.id}
          label="Inbox items"
        >
          {#snippet row(r)}
            {#if r.type === 'section'}
              <div class="section" role="heading" aria-level="2" data-section={r.id}>
                {r.label}
                <Badge>{r.count}</Badge>
              </div>
            {:else if r.type === 'group'}
              <div class="group" role="heading" aria-level="3" data-group={r.label}>
                {r.label} <span class="count">{r.count}</span>
              </div>
            {:else if r.type === 'empty'}
              <div class="empty">{r.label}</div>
            {:else}
              <button
                type="button"
                tabindex="-1"
                class="row"
                class:selected={cur === r}
                aria-current={cur === r ? 'true' : undefined}
                onclick={() => (selId = r.id)}
                ondblclick={() => void open(r)}
              >
                {#if r.type === 'ticket'}
                  <span class="key">{r.item.ticket.ref.key}</span>
                  <span class="ttl">{r.item.ticket.title}</span>
                  <Badge tone={statusTone(r.item.ticket.status.category)}>{r.item.ticket.status.name}</Badge>
                  <span class="meta">{relativeTime(Date.parse(r.item.ticket.updated_at))}</span>
                {:else if r.type === 'review'}
                  {@const ci = ciGlyph(r.item.review.ci)}
                  {@const dec = decisionInfo(r.item.review.decision)}
                  <span class="ci {ci.tone}" role="img" aria-label={ci.label} title={ci.label}
                    >{ci.glyph}</span
                  >
                  <span class="ttl">{r.item.review.title}</span>
                  {#if r.item.review.draft}<Badge>draft</Badge>{/if}
                  {#if dec}<Badge tone={dec.tone}>{dec.label}</Badge>{/if}
                  <span class="meta">{r.mine ? r.item.review.ref.repo : r.item.review.author.name}</span>
                  <span class="meta">{relativeTime(Date.parse(r.item.review.updated_at))}</span>
                {:else}
                  <Badge tone="danger">needs input</Badge>
                  <span class="ttl">{r.session.name}</span>
                  <span class="meta">{projects.byId(r.session.project_id)?.name ?? r.session.project_id}</span
                  >
                {/if}
              </button>
            {/if}
          {/snippet}
        </VirtualList>
      </div>
    {/if}
  {/if}

  <footer class="hints" aria-hidden="true">
    j/k move · Enter open · o browser · s start work / review locally · R refresh
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

  .list {
    flex: 1;
    min-height: 0;
  }

  .section,
  .group,
  .empty {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    height: 100%;
    padding: 0 var(--k-space-3);
  }

  .section {
    background: var(--k-bg-sunken);
    font-weight: 600;
  }

  .group {
    padding-left: var(--k-space-4);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .count {
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .empty {
    padding-left: var(--k-space-4);
    color: var(--k-fg-subtle);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    width: 100%;
    height: 100%;
    padding: 0 var(--k-space-3) 0 var(--k-space-5);
    border: 0;
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

  .ci {
    flex: none;
    width: 16px;
    text-align: center;
    font-weight: 700;
  }

  .ci.ok {
    color: var(--k-ok);
  }

  .ci.danger {
    color: var(--k-danger);
  }

  .ci.warn {
    color: var(--k-warn);
  }

  .ttl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    flex: none;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .hints {
    padding: var(--k-space-1) var(--k-space-3);
    border-top: 1px solid var(--k-border);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }
</style>
