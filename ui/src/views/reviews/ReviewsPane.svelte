<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError, ReviewItem, ReviewKind } from '$lib/gen';
  import { openExternal } from '$lib/ipc/commands';
  import { projects, reviews, toasts } from '$lib/stores';
  import { reviewKey } from '$lib/stores/reducers';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    Lamp,
    Select,
    Toggle,
    ROW_HEIGHT,
    VirtualList,
    relativeTime,
  } from '$lib/ui';

  import { ciGlyph, decisionInfo, isAuthError, myStateInfo, reviewPhase } from '../work/common';
  import { openContent } from '../work/nav';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { reviewLocally } from '../work/startWork';

  let { projectId, content, focused }: PaneProps<'reviews'> = $props();

  const scope = $derived(content.scope);
  const kinds: { kind: ReviewKind; label: string }[] = [
    { kind: 'review_requested', label: 'Review requested' },
    { kind: 'authored', label: 'My PRs' },
  ];

  const lists = $derived({
    review_requested: reviews.list(scope, 'review_requested'),
    authored: reviews.list(scope, 'authored'),
  });
  const bothLoading = $derived(
    (lists.review_requested.loading && !lists.review_requested.data) ||
      (lists.authored.loading && !lists.authored.data),
  );
  const noData = $derived(!lists.review_requested.data && !lists.authored.data);
  const firstError = $derived(lists.review_requested.error ?? lists.authored.error);
  const stale = $derived(lists.review_requested.stale || lists.authored.stale);
  const fetchedAt = $derived(lists.review_requested.fetchedAt ?? lists.authored.fetchedAt);
  const accountErrors = $derived.by(() => {
    const seen: Record<string, AccountError> = {};
    for (const l of [lists.review_requested, lists.authored])
      for (const e of l.data?.errors ?? []) seen[`${e.account_id}:${e.error.code}`] = e;
    return Object.values(seen);
  });

  $effect(() => {
    const s = scope;
    untrack(() => {
      void reviews.load(s, 'review_requested');
      void reviews.load(s, 'authored');
    });
  });

  // ---- filters ------------------------------------------------------------------------------
  let filter = $state('');
  let includeDrafts = $state(true);
  let repo = $state('');
  let filterInput = $state<HTMLInputElement>();

  const allItems = $derived([
    ...(lists.review_requested.data?.items ?? []),
    ...(lists.authored.data?.items ?? []),
  ]);
  const repos = $derived([...new Set(allItems.map((i) => i.review.ref.repo))].sort());

  function matches(i: ReviewItem): boolean {
    const r = i.review;
    if (!includeDrafts && r.draft) return false;
    if (repo !== '' && r.ref.repo !== repo) return false;
    const needle = filter.trim().toLowerCase();
    if (needle === '') return true;
    return `${r.title} ${r.ref.repo}#${r.ref.number} ${r.author.name} ${r.source_branch} ${r.linked_tickets.join(' ')}`
      .toLowerCase()
      .includes(needle);
  }

  type Row =
    | { type: 'header'; id: string; label: string; count: number }
    | { type: 'item'; id: string; item: ReviewItem };

  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    for (const k of kinds) {
      const items = (lists[k.kind].data?.items ?? []).filter(matches);
      out.push({ type: 'header', id: `h:${k.kind}`, label: k.label, count: items.length });
      for (const item of items)
        out.push({ type: 'item', id: `${k.kind}:${reviewKey(item.review.ref)}`, item });
    }
    return out;
  });
  const itemRows = $derived(rows.filter((r): r is Extract<Row, { type: 'item' }> => r.type === 'item'));
  const totalItems = $derived(allItems.length);

  // ---- selection ----------------------------------------------------------------------------
  let selId = $state<string | null>(null);
  const cur = $derived(itemRows.find((r) => r.id === selId) ?? itemRows[0] ?? null);
  let vlist = $state<{ scrollToIndex(i: number): void }>();
  let root = $state<HTMLDivElement>();

  $effect(() => {
    if (cur) vlist?.scrollToIndex(rows.indexOf(cur));
  });

  $effect(() => {
    if (focused && root && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
  });

  function step(delta: number): void {
    if (itemRows.length === 0) return;
    const at = cur ? itemRows.indexOf(cur) : -1;
    const next = itemRows[Math.min(itemRows.length - 1, Math.max(0, at + delta))];
    if (next) selId = next.id;
  }

  function refresh(): void {
    void reviews.load(scope, 'review_requested', true);
    void reviews.load(scope, 'authored', true);
  }

  function openDetail(item: ReviewItem): void {
    reviews.markSeen(item.review.ref);
    void openContent(projectId, { kind: 'review_detail', review: item.review.ref });
  }

  function browse(item: ReviewItem): void {
    openExternal({ url: item.review.url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  function onkeydown(e: KeyboardEvent): void {
    if ((e.target as HTMLElement).closest('input, textarea, select, [role="dialog"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const item = cur?.item;
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
        if (itemRows[0]) selId = itemRows[0].id;
        break;
      case 'End':
        if (itemRows.at(-1)) selId = itemRows.at(-1)!.id;
        break;
      case 'Enter':
        if (item) openDetail(item);
        break;
      case '/':
        filterInput?.focus();
        break;
      case 'R':
        refresh();
        break;
      case 'o':
        if (item) browse(item);
        break;
      case 's':
        if (item) void reviewLocally(item.review.ref, item.project_ids[0] ?? projectId);
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

  function chip(item: ReviewItem): string {
    const id = item.project_ids[0];
    return id ? (projects.byId(id)?.name ?? id) : 'Other';
  }
</script>

<!-- One list query per focus (not a get per row): it also brings back PRs updated after my review. -->
<svelte:window onfocus={() => void reviews.load(scope, 'review_requested', true)} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="k-listpane"
  data-testid="reviews-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label="Reviews"
  {onkeydown}
>
  <header class="k-toolbar">
    <input
      class="k-filter"
      bind:this={filterInput}
      bind:value={filter}
      placeholder="Filter (/)"
      aria-label="Filter reviews"
      onkeydown={filterKeys}
    />
    <!-- The tab and pane header already say "Reviews"; only the cross-project scope is news. -->
    {#if scope.kind === 'all'}<span class="title">All projects</span>{/if}
    <span class="spacer"></span>
    {#if repos.length > 1}
      <Select
        label="Repository"
        bind:value={repo}
        options={[{ value: '', label: 'All repos' }, ...repos.map((r) => ({ value: r, label: r }))]}
      />
    {/if}
    <Toggle label="Drafts" bind:checked={includeDrafts} />
    <Button variant="ghost" size="sm" icon="refresh-cw" chord="shift+r" onclick={refresh}>Refresh</Button>
  </header>

  {#if bothLoading && noData}
    <Loading label="Loading reviews" />
  {:else if noData && firstError}
    <ErrorState error={firstError} title="Could not load reviews" onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
          {isAuthError(firstError) ? 'Re-authenticate' : 'Open account settings'}
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
    {#if totalItems === 0}
      <EmptyState
        icon="git-pull-request"
        title="No review requests."
        body="Pull requests where you are a requested reviewer show up here."
      >
        {#snippet actions()}<Button onclick={refresh}>Refresh</Button>{/snippet}
      </EmptyState>
    {:else}
      <div class="list">
        <VirtualList
          bind:this={vlist}
          items={rows}
          itemHeight={ROW_HEIGHT}
          key={(r) => r.id}
          label="Pull requests"
        >
          {#snippet row(r)}
            {#if r.type === 'header'}
              <div class="k-group" role="heading" aria-level="2">
                {r.label}
                <span class="count k-num">{r.count}</span>
              </div>
            {:else}
              {@const rv = r.item.review}
              {@const ci = ciGlyph(rv.ci)}
              {@const dec = decisionInfo(rv.decision)}
              {@const mine = myStateInfo(rv.my_state)}
              <button
                type="button"
                tabindex="-1"
                class="k-row"
                class:selected={cur === r}
                aria-current={cur === r ? 'true' : undefined}
                data-key={reviewKey(rv.ref)}
                onclick={() => (selId = r.id)}
                ondblclick={() => openDetail(r.item)}
              >
                <span class="k-row-lamp"><Lamp level={ci.lamp} title={ci.label} /></span>
                <span class="k-row-key">#{rv.ref.number}</span>
                <span class="k-row-title">{rv.title}</span>
                <span class="k-row-tail">
                  {#if reviews.isNew(rv.ref)}<Badge tone="accent">new</Badge>{/if}
                  {#if rv.draft}<Badge>draft</Badge>{/if}
                  {#each rv.linked_tickets.slice(0, 2) as t (t)}<Badge tone="info">{t}</Badge>{/each}
                  {#if dec}<Badge tone={dec.tone}>{dec.label}</Badge>{/if}
                  {#if reviewPhase(rv) === 'updated'}<Badge tone="warn">Updated since your review</Badge>{/if}
                  {#if mine}<Badge tone={mine.tone}>{mine.label}</Badge>{/if}
                  <span class="k-narrow-hide"><Badge title={rv.ref.repo}>{chip(r.item)}</Badge></span>
                  <span class="k-row-meta k-narrow-hide">{rv.author.name}</span>
                  {#if rv.additions !== null || rv.deletions !== null}
                    <span class="k-row-meta k-mono"
                      ><span class="add">+{rv.additions ?? 0}</span>
                      <span class="del">−{rv.deletions ?? 0}</span></span
                    >
                  {/if}
                  <span class="k-row-meta">{relativeTime(Date.parse(rv.updated_at))}</span>
                </span>
              </button>
            {/if}
          {/snippet}
        </VirtualList>
      </div>
    {/if}
  {/if}

  <KeyHints
    hints={[
      ['j k', 'Move'],
      ['enter', 'Open'],
      ['/', 'Filter'],
      ['s', 'Review locally'],
      ['o', 'Open in browser'],
      ['shift+r', 'Refresh'],
    ]}
  />
</div>

<style>
  .add {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }
</style>
