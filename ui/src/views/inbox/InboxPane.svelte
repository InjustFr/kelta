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
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    IconButton,
    Lamp,
    VirtualList,
    relativeTime,
  } from '$lib/ui';

  import { ciGlyph, decisionInfo, isAuthError, statusTone } from '../work/common';
  import { openContent } from '../work/nav';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { reviewLocally, startWorkOnTicket } from '../work/startWork';
  import { groupByProject } from './groups';

  let { projectId, focused }: PaneProps<'inbox'> = $props();

  const ROW_HEIGHT = 26;
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
    | { type: 'section'; id: string; label: string; count: number; lamp?: 'needs_input' }
    | { type: 'group'; id: string; label: string; count: number }
    | { type: 'empty'; id: string; label: string }
    | { type: 'ticket'; id: string; item: TicketItem }
    | { type: 'review'; id: string; item: ReviewItem; mine: boolean }
    | { type: 'session'; id: string; session: SessionInfo };

  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    // Claude waiting comes first: it is the one thing that blocks work.
    out.push({
      type: 'section',
      id: 's:input',
      label: 'Needs input',
      count: needing.length,
      lamp: 'needs_input',
    });
    if (needing.length === 0) out.push({ type: 'empty', id: 'e:input', label: 'No session needs input.' });
    for (const session of needing) out.push({ type: 'session', id: `n:${session.id}`, session });

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
    if (!focused || cur?.type !== 'ticket') return;
    selectTicket(cur.item.ticket.ref, cur.item.project_ids[0] ?? projectId);
    return () => selectTicket(null, null);
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
  class="k-listpane"
  data-testid="inbox-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Inbox"
  {onkeydown}
>
  <header class="k-toolbar">
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
      <EmptyState icon="inbox" title="No tickets, review requests or sessions need you.">
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
              <div class="k-group section" role="heading" aria-level="2" data-section={r.id}>
                {#if r.lamp}<span class="k-row-lamp"><Lamp level={r.lamp} /></span>{/if}
                {r.label}
                <span class="count k-num">{r.count}</span>
              </div>
            {:else if r.type === 'group'}
              <div class="group" role="heading" aria-level="3" data-group={r.label}>
                {r.label} <span class="count k-num">{r.count}</span>
              </div>
            {:else if r.type === 'empty'}
              <div class="empty">{r.label}</div>
            {:else}
              <button
                type="button"
                tabindex="-1"
                class="k-row"
                class:selected={cur === r}
                aria-current={cur === r ? 'true' : undefined}
                onclick={() => (selId = r.id)}
                ondblclick={() => void open(r)}
              >
                {#if r.type === 'ticket'}
                  <span class="k-row-lamp"></span>
                  <span class="k-row-key">{r.item.ticket.ref.key}</span>
                  <span class="k-row-title">{r.item.ticket.title}</span>
                  <Badge tone={statusTone(r.item.ticket.status.category)}>{r.item.ticket.status.name}</Badge>
                  <span class="k-row-meta">{relativeTime(Date.parse(r.item.ticket.updated_at))}</span>
                {:else if r.type === 'review'}
                  {@const ci = ciGlyph(r.item.review.ci)}
                  {@const dec = decisionInfo(r.item.review.decision)}
                  <span class="k-row-lamp"><Lamp level={ci.lamp} title={ci.label} /></span>
                  <span class="k-row-key">#{r.item.review.ref.number}</span>
                  <span class="k-row-title">{r.item.review.title}</span>
                  {#if r.item.review.draft}<Badge>draft</Badge>{/if}
                  {#if dec}<Badge tone={dec.tone}>{dec.label}</Badge>{/if}
                  <span class="k-row-meta">{r.mine ? r.item.review.ref.repo : r.item.review.author.name}</span
                  >
                  <span class="k-row-meta">{relativeTime(Date.parse(r.item.review.updated_at))}</span>
                {:else}
                  <span class="k-row-lamp"><Lamp level="needs_input" /></span>
                  <span class="k-row-key">{r.session.name}</span>
                  <span class="k-row-title"
                    >{projects.byId(r.session.project_id)?.name ?? r.session.project_id}</span
                  >
                {/if}
              </button>
            {/if}
          {/snippet}
        </VirtualList>
      </div>
    {/if}
  {/if}

  <KeyHints
    hints={[
      ['j/k', 'move'],
      ['Enter', 'open'],
      ['o', 'browser'],
      ['s', 'start work or review locally'],
      ['R', 'refresh'],
    ]}
  />
</div>

<style>
  .title {
    font-weight: var(--k-weight-strong);
  }

  .spacer {
    flex: 1;
  }

  .list {
    flex: 1;
    min-height: 0;
  }

  .section {
    color: var(--k-fg);
  }

  .group,
  .empty {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: 100%;
    padding: 0 var(--k-space-3) 0 calc(var(--k-space-3) * 2 + 10px);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .empty {
    color: var(--k-fg-subtle);
  }

  .count {
    font-size: var(--k-font-size-xs);
    font-weight: 400;
    color: var(--k-fg-subtle);
  }
</style>
