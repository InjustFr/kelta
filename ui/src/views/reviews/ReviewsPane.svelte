<script lang="ts">
  import { msg, t } from '$lib/i18n/index.svelte';
  import { tick, untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError, ReviewItem, ReviewKind, WorkItem } from '$lib/gen';
  import { openExternal, reviewNudge } from '$lib/ipc/commands';
  import { GLOBAL, projects, reviews, settings, tags, tickets, toasts, ui, work } from '$lib/stores';
  import { reviewKey } from '$lib/stores/reducers';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    Lamp,
    Menu,
    Select,
    Tabs,
    Toggle,
    ROW_HEIGHT,
    VirtualList,
    relativeTime,
    type MenuItem,
  } from '$lib/ui';

  import { runWorkAction } from '../work/actions';
  import { ciGlyph, decisionInfo, isAuthError, myStateInfo, reviewPhase } from '../work/common';
  import { openFrom, persistPaneContent } from '../work/nav';
  import { groupByTicket } from './group';
  import LinkedTicket from './LinkedTicket.svelte';
  import ReviewDetailPane from './ReviewDetailPane.svelte';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { reviewLocally } from '../work/startWork';
  import TagChips from '../tags/TagChips.svelte';
  import TagFilter from '../tags/TagFilter.svelte';
  import TagMenu from '../tags/TagMenu.svelte';

  import { toggleDraft } from './draft';
  import { nudgedRecently, pingBody, waitingChip } from './nudge';

  let { projectId, tabId, paneId, content, focused }: PaneProps<'reviews'> = $props();
  /** The pane's project; `null` in the rail's Reviews view, where each PR uses its own project. */
  const home = $derived(projectId === GLOBAL ? null : projectId);

  const scope = $derived(content.scope);
  const TABS: { kind: ReviewKind; label: string }[] = $derived([
    { kind: 'review_requested', label: t('For me') },
    { kind: 'team', label: t('Team') },
    { kind: 'authored', label: t('Mine') },
  ]);
  /** Team spans the repos of the projects a ticket source feeds: in a project's pane, only when one feeds it. */
  const sourceList = $derived(settings.value()?.sources ?? []);
  const teamOffered = $derived(
    scope.kind !== 'project' || sourceList.some((s) => s.projects.includes(scope.id)),
  );
  const tabs = $derived(TABS.filter((t) => t.kind !== 'team' || teamOffered));
  let tabOverride = $state<ReviewKind | null>(null);
  const tab = $derived.by((): ReviewKind => {
    const t = tabOverride ?? content.tab ?? 'review_requested';
    return tabs.some((x) => x.kind === t) ? t : 'review_requested';
  });

  function setTab(k: ReviewKind): void {
    tabOverride = k;
    // The core polls Team only while a visible pane shows it (its `tab`).
    persistPaneContent(projectId, paneId, { tab: k });
  }

  const lists = $derived({
    review_requested: reviews.list(scope, 'review_requested'),
    team: reviews.list(scope, 'team'),
    authored: reviews.list(scope, 'authored'),
  });
  const shown = $derived(lists[tab]);
  const accountErrors = $derived.by(() => {
    const seen: Record<string, AccountError> = {};
    for (const e of shown.data?.errors ?? []) seen[`${e.account_id}:${e.error.code}`] = e;
    return Object.values(seen);
  });

  // For me and Mine always (their counts, `p` hand-offs); Team only while shown.
  $effect(() => {
    const s = scope;
    untrack(() => {
      void reviews.load(s, 'review_requested');
      void reviews.load(s, 'authored');
    });
  });
  $effect(() => {
    const s = scope;
    if (tab === 'team') untrack(() => void reviews.load(s, 'team'));
  });

  // ---- filters ------------------------------------------------------------------------------
  let filter = $state('');
  let includeDrafts = $state(true);
  let repo = $state('');
  let filterInput = $state<HTMLInputElement>();
  /** The toolbar's tag chip (#349). shortcut: per mount, not persisted. */
  let tagFilter = $state<string | null>(null);

  const allItems = $derived(shown.data?.items ?? []);
  const repos = $derived([...new Set(allItems.map((i) => i.review.ref.repo))].sort());
  /** `G` on Team: rows grouped by their first linked ticket. shortcut: per mount, not persisted. */
  let grouped = $state(false);
  /** My open PRs (Mine lists them all): Team rows read "you" as their author. */
  const mineKeys = $derived(new Set((lists.authored.data?.items ?? []).map((i) => reviewKey(i.review.ref))));
  /** No source feeds a project (of the scope) bound to a repository: Team has nothing to list. */
  const noTeamReach = $derived(
    settings.value() !== null &&
      !sourceList.some((s) =>
        s.projects.some(
          (p) =>
            (scope.kind !== 'project' || p === scope.id) &&
            !!projects.byId(p)?.repos.some((r) => r.code_host),
        ),
      ),
  );

  function matches(i: ReviewItem): boolean {
    const r = i.review;
    if (!includeDrafts && r.draft) return false;
    if (repo !== '' && r.ref.repo !== repo) return false;
    const tag = tags.live(tagFilter);
    if (tag && !tags.has('prs', reviewKey(r.ref), tag)) return false;
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
    const items = allItems.filter(matches);
    const row = (item: ReviewItem): Row => ({
      type: 'item',
      id: `${tab}:${reviewKey(item.review.ref)}`,
      item,
    });
    if (tab !== 'team' || !grouped) return items.map(row);
    const loaded = Object.values(tickets.lists).flatMap((l) => l.data?.items ?? []);
    return groupByTicket(items, loaded).flatMap((g): Row[] => [
      {
        type: 'header',
        id: `h:${g.key ?? ''}`,
        label: g.key === null ? t('No ticket') : g.title ? `${g.key} ${g.title}` : g.key,
        count: g.items.length,
      },
      ...g.items.map(row),
    ]);
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

  // `p` on a ticket in the Tickets view (`ui.select`) lands on its PR here: For me, else Mine, else Team.
  $effect(() => {
    const want = ui.select;
    if (want?.view !== 'reviews' || home !== null) return;
    const holds = (k: ReviewKind) => lists[k].data?.items.some((i) => reviewKey(i.review.ref) === want.key);
    const k = (['review_requested', 'authored', 'team'] as const).find(holds);
    if (k) {
      if (k !== untrack(() => tab)) setTab(k);
      selId = `${k}:${want.key}`;
    }
    ui.select = null;
  });

  // ---- split view: the selected PR's detail in a column on the right (as Tickets, T3) ----------
  // shortcut: on/off is per mount, not persisted, as in Tickets.
  let split = $state(true);
  let paneWidth = $state(0);
  let side = $state<HTMLElement>();
  /** Below ~720px the list would be too narrow: the detail opens as its own pane instead. */
  const wide = $derived(paneWidth >= 720);
  const splitShown = $derived(split && wide);

  /** Enter: the detail column takes the keyboard (narrow pane: the standalone pane). */
  async function focusDetail(item: ReviewItem): Promise<void> {
    if (!wide) return openDetail(item);
    split = true;
    await tick();
    side?.querySelector<HTMLElement>('[data-testid="review-detail"]')?.focus();
  }

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
    if (tab === 'team') void reviews.load(scope, 'team', true);
  }

  function openDetail(item: ReviewItem): void {
    reviews.markSeen(item.review.ref);
    void openFrom(projectId, item.project_ids, { kind: 'review_detail', review: item.review.ref });
  }

  /** My PR's work item (by `pr_url`), for Merge when ready. */
  function workOf(item: ReviewItem): WorkItem | null {
    return work.all.find((w) => w.pr_url === item.review.url && w.state.kind !== 'finished') ?? null;
  }

  function mergeWhenReady(item: ReviewItem): void {
    const w = item.review.kind === 'authored' ? workOf(item) : null;
    if (w) void runWorkAction('merge', w);
    else toasts.info(t('Merge when ready: only for my PRs with a work item'));
  }

  function browse(item: ReviewItem): void {
    openExternal({ url: item.review.url }).catch((err) => toasts.error(err, t('Open in browser')));
  }

  // ---- local tags (#349) ----------------------------------------------------------------------
  let tagMenu = $state<{ x: number; y: number; key: string } | null>(null);

  function openTags(item: ReviewItem): void {
    const r = root?.querySelector('.k-row.selected')?.getBoundingClientRect();
    tagMenu = { x: (r?.left ?? 16) + 24, y: (r?.bottom ?? 40) + 2, key: reviewKey(item.review.ref) };
  }

  // ---- nudge (my PRs) -------------------------------------------------------------------------
  let nudgeMenu = $state<{ x: number; y: number; item: ReviewItem } | null>(null);
  const slaHours = $derived(settings.value()?.reviews.sla_hours ?? 24);

  function openNudge(item: ReviewItem): void {
    if (item.review.kind !== 'authored') return;
    const r = root?.querySelector('.k-row.selected')?.getBoundingClientRect();
    nudgeMenu = { x: r ? r.right - 220 : 40, y: (r?.bottom ?? 40) + 2, item };
  }

  const nudgeItems = $derived.by((): MenuItem[] => {
    const rv = nudgeMenu?.item.review;
    const why =
      rv?.nudged_at && nudgedRecently(rv)
        ? t('Nudged {ago}; again 24 h after', { ago: relativeTime(Date.parse(rv.nudged_at)) })
        : undefined;
    return [
      {
        id: 'rerequest',
        label: t('Re-request review'),
        key: 'r',
        kbd: 'r',
        disabled: !!why || !rv?.waiting_on.length,
        title: why,
      },
      { id: 'ping', label: t('Comment ping'), key: 'c', kbd: 'c', disabled: !!why, title: why },
    ];
  });

  async function nudge(id: string): Promise<void> {
    const item = nudgeMenu?.item;
    if (!item) return;
    const template = settings.value()?.reviews.nudge_template ?? '{reviewers}';
    const comment = id === 'ping' ? pingBody(template, item.review) : null;
    try {
      await reviewNudge({ review: item.review.ref, who: item.review.waiting_on, comment });
      toasts.info(id === 'ping' ? t('Ping posted') : t('Review re-requested'));
    } catch (err) {
      toasts.error(err, t('Nudge'));
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, select, [role="dialog"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // The detail column runs its own keys; Esc hands the keyboard back to the list.
    if (side?.contains(target)) {
      if (e.key === 'Escape') {
        root?.focus();
        e.preventDefault();
      }
      return;
    }
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
        if (item && e.shiftKey) openDetail(item);
        else if (item) void focusDetail(item);
        break;
      case ' ':
        if (wide) split = !split;
        else if (item) openDetail(item);
        break;
      case '/':
        filterInput?.focus();
        break;
      case 'R':
        refresh();
        break;
      case '1':
      case '2':
      case '3': {
        const t = tabs[Number(e.key) - 1];
        if (t) setTab(t.kind);
        break;
      }
      case 'G':
        if (tab !== 'team') return;
        grouped = !grouped;
        break;
      case 'o':
        if (item) browse(item);
        break;
      case 'M':
        if (item) mergeWhenReady(item);
        break;
      case 's':
        if (item) void reviewLocally(item.review.ref, home ?? item.project_ids[0]);
        break;
      case 'n':
        if (item) openNudge(item);
        break;
      case 'd':
        if (item) void toggleDraft(item.review);
        break;
      case 't':
        if (item) openTags(item);
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
    return id ? (projects.byId(id)?.name ?? id) : t('Other');
  }
</script>

<!-- One list query per focus (not a get per row): it also brings back PRs updated after my review. -->
<svelte:window onfocus={() => void reviews.load(scope, 'review_requested', true)} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="k-listpane"
  data-testid="reviews-pane"
  bind:this={root}
  bind:clientWidth={paneWidth}
  tabindex="0"
  role="group"
  aria-label={t('Reviews')}
  {onkeydown}
>
  <header class="k-toolbar">
    <input
      class="k-filter"
      bind:this={filterInput}
      bind:value={filter}
      placeholder={t('Filter (/)')}
      aria-label={t('Filter reviews')}
      onkeydown={filterKeys}
    />
    <Tabs
      label={t('Reviews')}
      items={tabs.map((x) => {
        const n = lists[x.kind].data?.items.length;
        return { id: x.kind, label: n === undefined ? x.label : `${x.label} ${n}` };
      })}
      value={tab}
      onchange={setTab}
    />
    <!-- The tab and pane header already say "Reviews"; only the cross-project scope is news. -->
    {#if scope.kind === 'all'}<span class="title">{t('All projects')}</span>{/if}
    <TagFilter
      kind="prs"
      keys={allItems.map((i) => reviewKey(i.review.ref))}
      value={tagFilter}
      onchange={(name) => (tagFilter = name)}
    />
    <span class="spacer"></span>
    {#if repos.length > 1}
      <Select
        label={t('Repository')}
        bind:value={repo}
        options={[{ value: '', label: t('All repos') }, ...repos.map((r) => ({ value: r, label: r }))]}
      />
    {/if}
    <Toggle label={t('Drafts')} bind:checked={includeDrafts} />
    <Button variant="ghost" size="sm" icon="refresh-cw" chord="shift+r" onclick={refresh}
      >{t('Refresh')}</Button
    >
  </header>

  {#if shown.loading && !shown.data}
    <Loading label={t('Loading reviews')} />
  {:else if !shown.data && shown.error}
    <ErrorState error={shown.error} title={t('Could not load reviews')} onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
          {isAuthError(shown.error) ? t('Re-authenticate') : t('Open account settings')}
        </Button>
      {/snippet}
    </ErrorState>
  {:else}
    <StateBanner
      stale={shown.stale}
      fetchedAt={shown.fetchedAt}
      error={shown.data ? shown.error : null}
      errors={accountErrors}
      onretry={refresh}
    />
    {#if totalItems === 0 && tab === 'team' && noTeamReach}
      <EmptyState icon="git-pull-request" title={t('No ticket source feeds a project with a repository yet')}>
        {#snippet actions()}<Button
            variant="primary"
            onclick={() =>
              ui.openSheet('tracker.source_picker', {
                projectId: scope.kind === 'project' ? scope.id : undefined,
              })}>{t('Add a ticket source')}</Button
          >{/snippet}
      </EmptyState>
    {:else if totalItems === 0}
      <EmptyState
        icon="git-pull-request"
        title={tab === 'team'
          ? t("No open PRs on your sources' repos")
          : tab === 'authored'
            ? t('No open PRs of yours.')
            : t('No review requests.')}
        body={tab === 'review_requested'
          ? t('Pull requests where you are a requested reviewer show up here.')
          : undefined}
      >
        {#snippet actions()}<Button onclick={refresh}>{t('Refresh')}</Button>{/snippet}
      </EmptyState>
    {:else}
      <div class="k-split">
        <div class="list">
          <VirtualList
            bind:this={vlist}
            items={rows}
            itemHeight={ROW_HEIGHT}
            key={(r) => r.id}
            label={t('Pull requests')}
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
                {@const wait = waitingChip(r.item, slaHours)}
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
                    <TagChips kind="prs" itemKey={reviewKey(rv.ref)} />
                    {#if reviews.isNew(rv.ref)}<Badge tone="accent">{t('new')}</Badge>{/if}
                    {#if rv.draft}<Badge>{t('draft')}</Badge>{/if}
                    {#if workOf(r.item)?.auto_finish}<Badge tone="accent">{t('armed')}</Badge>{/if}
                    {#each rv.linked_tickets.slice(0, 2) as key (key)}<LinkedTicket
                        ticketKey={key}
                        projectId={home}
                        repo={rv.ref.repo}
                        tabindex={-1}
                      />{/each}
                    {#if dec}<Badge tone={dec.tone}>{dec.label}</Badge>{/if}
                    {#if reviewPhase(rv) === 'updated'}<Badge tone="warn"
                        >{t('Updated since your review')}</Badge
                      >{/if}
                    {#if mine}<Badge tone={mine.tone}>{mine.label}</Badge>{/if}
                    {#if wait}<Badge tone={wait.late ? 'warn' : 'neutral'} title={wait.title}
                        >{wait.label}</Badge
                      >{/if}
                    <span class="k-narrow-hide"><Badge title={rv.ref.repo}>{chip(r.item)}</Badge></span>
                    <span class="k-row-meta k-narrow-hide"
                      >{rv.kind !== 'team'
                        ? rv.author.name
                        : mineKeys.has(reviewKey(rv.ref))
                          ? t('you')
                          : `@${rv.author.login ?? rv.author.name}`}</span
                    >
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
        {#if splitShown}
          <aside class="k-split-side" bind:this={side} aria-label={t('Review detail')}>
            {#if cur}
              <!-- The PR's project runs its actions (fix, review locally); a PR no project binds uses the active one. -->
              {@const pid = home ?? cur.item.project_ids[0] ?? projects.activeId}
              {#if pid}
                <!-- Keyed: the "PR changed" banner and an open dialog belong to one PR. -->
                {#key cur.id}<ReviewDetailPane
                    projectId={pid}
                    {tabId}
                    {paneId}
                    content={{ kind: 'review_detail', review: cur.item.review.ref }}
                    visible
                    focused={false}
                    embedded
                  />{/key}
              {/if}
            {:else}
              <p class="k-split-empty">{t('Select a pull request to see it here.')}</p>
            {/if}
          </aside>
        {/if}
      </div>
    {/if}
  {/if}

  <KeyHints
    hints={[
      ['j k', msg('Move')],
      ['1 2 3', msg('Tab')],
      ...(tab === 'team' ? [['shift+g', msg('Group by ticket')] as [string, string]] : []),
      ['space', msg('Split')],
      ['enter', msg('Open')],
      ['shift+enter', msg('Own pane')],
      ['/', msg('Filter')],
      ['s', msg('Review locally')],
      ['n', msg('Nudge (my PRs)')],
      ['d', msg('Mark ready / draft (my PRs)')],
      ['t', msg('Tags')],
      ['o', msg('Open in browser')],
      ['shift+m', msg('Merge when ready')],
      ['shift+r', msg('Refresh')],
    ]}
  />
</div>

{#if tagMenu}
  <TagMenu
    kind="prs"
    itemKey={tagMenu.key}
    x={tagMenu.x}
    y={tagMenu.y}
    onclose={() => {
      tagMenu = null;
      root?.focus();
    }}
  />
{/if}
{#if nudgeMenu}
  <Menu
    items={nudgeItems}
    x={nudgeMenu.x}
    y={nudgeMenu.y}
    label={t('Nudge reviewers')}
    onselect={(id) => void nudge(id)}
    onclose={() => {
      nudgeMenu = null;
      root?.focus();
    }}
  />
{/if}

<style>
  .add {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }
</style>
