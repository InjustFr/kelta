<script lang="ts">
  import { msg, t } from '$lib/i18n/index.svelte';
  import { tick, untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AccountError, Review, ReviewItem, ReviewKind, WorkItem } from '$lib/gen';
  import { openExternal, reviewNudge } from '$lib/ipc/commands';
  import { GLOBAL, projects, reviews, settings, tags, toasts, ui, work } from '$lib/stores';
  import { inflight } from '$lib/stores/inflight.svelte';
  import { reviewKey } from '$lib/stores/reducers';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    Icon,
    LoadBar,
    Menu,
    Select,
    Spinner,
    ROW_HEIGHT,
    VirtualList,
    relativeTime,
    type MenuItem,
  } from '$lib/ui';

  import { runWorkAction } from '../work/actions';
  import { isAuthError, reviewPhase } from '../work/common';
  import { openFrom } from '../work/nav';
  import ReviewDetailPane from './ReviewDetailPane.svelte';
  import ActionMenu, { below, under, type RowAction } from '../work/shared/ActionMenu.svelte';
  import { onActions } from '../work/ui.svelte';
  import KeyHints from '../work/shared/KeyHints.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { reviewLocally } from '../work/startWork';
  import { pickPrWorkflow } from '../tickets/workflow.svelte';
  import RowButton from '../tickets/RowButton.svelte';
  import TagChips from '../tags/TagChips.svelte';
  import TagFilter from '../tags/TagFilter.svelte';
  import TagMenu from '../tags/TagMenu.svelte';
  import FilterBar from '../filters/FilterBar.svelte';
  import { clearAll, commonFilters, noMatch, type ActiveFilter } from '../filters/filters';

  import ChangeMap from './ChangeMap.svelte';
  import { AREA_LABEL, AREAS, changeMap } from './changemap';
  import { draftBlocked, draftLabel, toggleDraft } from './draft';
  import { nudgedRecently, pingBody } from './nudge';
  import {
    SECTIONS,
    STALE_DAYS,
    openDays,
    rowState,
    sectionItems,
    stackOrder,
    type SectionId,
    type Stacked,
  } from './sections';

  let { projectId, tabId, paneId, content, focused }: PaneProps<'reviews'> = $props();
  /** The pane's project; `null` in the rail's Reviews view, where each PR uses its own project. */
  const home = $derived(projectId === GLOBAL ? null : projectId);

  const scope = $derived(content.scope);
  /** Team spans the repos of the projects a ticket source feeds: in a project's pane, only when one feeds it. */
  const sourceList = $derived(settings.value()?.sources ?? []);
  const teamOffered = $derived(
    scope.kind !== 'project' || sourceList.some((s) => s.projects.includes(scope.id)),
  );

  const lists = $derived({
    review_requested: reviews.list(scope, 'review_requested'),
    team: reviews.list(scope, 'team'),
    authored: reviews.list(scope, 'authored'),
  });
  /** The lists this pane shows (#401: every section on one list). */
  const shown = $derived([lists.review_requested, lists.authored, ...(teamOffered ? [lists.team] : [])]);
  const noData = $derived(shown.every((l) => !l.data));
  const firstError = $derived(shown.find((l) => l.error)?.error ?? null);
  const accountErrors = $derived.by(() => {
    const seen: Record<string, AccountError> = {};
    for (const l of shown) for (const e of l.data?.errors ?? []) seen[`${e.account_id}:${e.error.code}`] = e;
    return Object.values(seen);
  });
  const fetchedAt = $derived(
    Math.min(...shown.map((l) => l.fetchedAt ?? Infinity).filter((x) => x !== Infinity)),
  );

  $effect(() => {
    const s = scope;
    const team = teamOffered;
    untrack(() => {
      void reviews.load(s, 'review_requested');
      void reviews.load(s, 'authored');
      if (team) void reviews.load(s, 'team');
    });
  });

  // ---- filters ------------------------------------------------------------------------------
  let filter = $state('');
  let repo = $state('');
  let filterInput = $state<HTMLInputElement>();
  /** The toolbar's tag chip (#349). shortcut: per mount, not persisted. */
  let tagFilter = $state<string | null>(null);

  const items = (k: ReviewKind): ReviewItem[] => lists[k].data?.items ?? [];
  const allItems = $derived([...items('review_requested'), ...items('team'), ...items('authored')]);
  const repos = $derived([...new Set(allItems.map((i) => i.review.ref.repo))].sort());
  /** No source feeds a project (of the scope) bound to a repository: the team sections stay empty. */
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

  const activeFilters = $derived<ActiveFilter[]>([
    ...commonFilters(
      filter,
      () => (filter = ''),
      tagFilter,
      () => (tagFilter = null),
    ),
    ...(repo !== ''
      ? [{ id: 'repo', label: t('Repository: {name}', { name: repo }), clear: () => (repo = '') }]
      : []),
  ]);

  function matches(i: ReviewItem): boolean {
    const r = i.review;
    if (repo !== '' && r.ref.repo !== repo) return false;
    const tag = tags.live(tagFilter);
    if (tag && !tags.has('prs', reviewKey(r.ref), tag)) return false;
    const needle = filter.trim().toLowerCase();
    if (needle === '') return true;
    return `${r.title} ${r.ref.repo}#${r.ref.number} ${r.author.name} ${r.source_branch} ${r.linked_tickets.join(' ')}`
      .toLowerCase()
      .includes(needle);
  }

  // ---- sections (#401) --------------------------------------------------------------------------
  type Line =
    | { type: 'section'; id: string; section: SectionId; label: string; count: number; open: boolean }
    | { type: 'none'; id: string; text: string }
    | { type: 'row'; id: string; s: Stacked; section: SectionId };

  /** Sections whose fold the user flipped from its default. */
  let flipped = $state<SectionId[]>([]);
  const isOpen = (id: SectionId): boolean =>
    activeFilters.length > 0 || !!SECTIONS.find((s) => s.id === id)?.folded === flipped.includes(id);
  const toggleSection = (id: SectionId): void => {
    flipped = flipped.includes(id) ? flipped.filter((x) => x !== id) : [...flipped, id];
  };

  const parts = $derived(
    sectionItems(
      items('review_requested').filter(matches),
      teamOffered ? items('team').filter(matches) : [],
      items('authored').filter(matches),
    ),
  );

  const lines = $derived.by((): Line[] => {
    const out: Line[] = [];
    for (const sec of SECTIONS) {
      const rows = stackOrder(parts[sec.id]);
      const asked = sec.id === 'asked' && activeFilters.length === 0 && !!lists.review_requested.data;
      if (rows.length === 0 && !asked) continue;
      const open = isOpen(sec.id);
      out.push({
        type: 'section',
        id: `s:${sec.id}`,
        section: sec.id,
        label: sec.label,
        count: rows.length,
        open,
      });
      if (!open) continue;
      if (rows.length === 0)
        out.push({ type: 'none', id: `n:${sec.id}`, text: t('Nobody has asked you for a review.') });
      for (const s of rows)
        out.push({ type: 'row', id: `${sec.id}:${reviewKey(s.item.review.ref)}`, s, section: sec.id });
    }
    return out;
  });
  const itemRows = $derived(lines.filter((l): l is Extract<Line, { type: 'row' }> => l.type === 'row'));

  // ---- selection ----------------------------------------------------------------------------
  let selId = $state<string | null>(null);
  const cur = $derived(itemRows.find((r) => r.id === selId) ?? itemRows[0] ?? null);
  let vlist = $state<{ scrollToIndex(i: number): void }>();
  let root = $state<HTMLDivElement>();
  let side = $state<HTMLElement>();

  $effect(() => {
    if (cur) vlist?.scrollToIndex(lines.indexOf(cur));
  });

  // `p` on a ticket in the Tickets view (`ui.select`) lands on its PR here, its section opened.
  $effect(() => {
    const want = ui.select;
    if (want?.view !== 'reviews' || home !== null) return;
    const sec = SECTIONS.find((s) => parts[s.id].some((i) => reviewKey(i.review.ref) === want.key));
    if (sec) {
      if (!untrack(() => isOpen(sec.id))) toggleSection(sec.id);
      selId = `${sec.id}:${want.key}`;
    }
    ui.select = null;
  });

  /** Enter: the brief beside the list (under it in a narrow pane) takes the keyboard. */
  async function focusDetail(): Promise<void> {
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
    if (teamOffered) void reviews.load(scope, 'team', true);
  }

  function openDetail(item: ReviewItem): void {
    reviews.markSeen(item.review.ref);
    void openFrom(projectId, item.project_ids, { kind: 'review_detail', review: item.review.ref });
  }

  /** My PR's work item (by `pr_url`), for Merge when ready. */
  function workOf(item: ReviewItem): WorkItem | null {
    return work.all.find((w) => w.pr_url === item.review.url && w.state.kind !== 'finished') ?? null;
  }

  const draft = (rv: Review) => inflight.track(reviewKey(rv.ref), () => toggleDraft(rv));

  function mergeWhenReady(item: ReviewItem): void {
    const w = item.review.kind === 'authored' ? workOf(item) : null;
    if (w) void inflight.track(reviewKey(item.review.ref), () => runWorkAction('merge', w));
    else toasts.info(t('Merge when ready: only for my PRs with a work item'));
  }

  function browse(item: ReviewItem): void {
    openExternal({ url: item.review.url }).catch((err) => toasts.error(err, t('Open in browser')));
  }

  const selectedRect = () => root?.querySelector('.row.selected')?.getBoundingClientRect();

  // ---- local tags (#349) ----------------------------------------------------------------------
  let tagMenu = $state<{ x: number; y: number; key: string } | null>(null);

  function openTags(item: ReviewItem): void {
    const r = selectedRect();
    tagMenu = { x: (r?.left ?? 16) + 24, y: (r?.bottom ?? 40) + 2, key: reviewKey(item.review.ref) };
  }

  // ---- nudge (my PRs) -------------------------------------------------------------------------
  let nudgeMenu = $state<{ x: number; y: number; item: ReviewItem } | null>(null);

  function openNudge(item: ReviewItem): void {
    if (item.review.kind !== 'authored' || item.review.assigned) return;
    const r = selectedRect();
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
      await inflight.track(reviewKey(item.review.ref), () =>
        reviewNudge({ review: item.review.ref, who: item.review.waiting_on, comment }),
      );
      toasts.info(id === 'ping' ? t('Ping posted') : t('Review re-requested'));
    } catch (err) {
      toasts.error(err, t('Nudge'));
    }
  }

  /** The full key list (`?`); the footer shows the first five. */
  let keysOpen = $state(false);
  const hints = $derived<[string, string][]>([
    ['j k', msg('Move')],
    ['enter', msg('Open')],
    ['s', msg('Review locally')],
    ['/', msg('Filter')],
    ['shift+enter', msg('Own pane')],
    ['n', msg('Nudge (my PRs)')],
    ['d', msg('Mark ready / draft (my PRs)')],
    ['t', msg('Tags')],
    ['w', msg('Run workflow')],
    ['o', msg('Open in browser')],
    ['shift+m', msg('Merge when ready')],
    ['shift+r', msg('Refresh')],
  ]);

  function onkeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement;
    if (target.closest('input, textarea, select, [role="dialog"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    // The brief runs its own keys; Esc hands the keyboard back to the list.
    if (side?.contains(target)) {
      if (e.key === 'Escape') {
        root?.focus();
        e.preventDefault();
      }
      return;
    }
    const item = cur?.s.item;
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
      case ' ':
        if (item && e.shiftKey) openDetail(item);
        else if (item) void focusDetail();
        break;
      case '/':
        filterInput?.focus();
        break;
      case '?':
        keysOpen = true;
        break;
      case 'R':
        refresh();
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
        if (item) void draft(item.review);
        break;
      case 't':
        if (item) openTags(item);
        break;
      case 'w':
        if (item) pickPrWorkflow(item.review.ref, home ?? item.project_ids[0] ?? null);
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  // ---- the row's actions (#397): every key in the `⋯` menu (right click too) -----------------
  let more = $state.raw<{ x: number; y: number; actions: RowAction[]; lit?: boolean } | null>(null);

  /** `⌘.` (#404), from the list or the brief: the selected row's menu, under its `⋯`. */
  function menuKey(): boolean {
    if (!cur) return false;
    more = { ...under(root?.querySelector('.row.selected .acts')), actions: rowActions(cur.s.item), lit: true };
    return true;
  }

  function rowActions(item: ReviewItem): RowAction[] {
    const rv = item.review;
    const pid = home ?? item.project_ids[0] ?? null;
    return [
      { id: 'open', label: t('Open review'), key: 'enter', run: () => focusDetail() },
      { id: 'pane', label: t('Open in own pane'), key: 'shift+enter', run: () => openDetail(item) },
      {
        id: 'local',
        label: t('Review locally'),
        key: 's',
        run: () => reviewLocally(rv.ref, pid ?? undefined),
      },
      { id: 'workflow', label: t('Run workflow…'), key: 'w', run: () => pickPrWorkflow(rv.ref, pid) },
      { id: 'tags', label: t('Tags…'), key: 't', run: () => openTags(item) },
      { id: 'browser', label: t('Open in browser'), key: 'o', run: () => browse(item) },
      {
        id: 'nudge',
        label: t('Nudge reviewers'),
        key: 'n',
        reason: rv.kind === 'authored' && !rv.assigned ? null : t('Only on your own pull requests'),
        run: () => openNudge(item),
      },
      { id: 'draft', label: draftLabel(rv), key: 'd', reason: draftBlocked(rv), run: () => draft(rv) },
      {
        id: 'merge',
        label: t('Merge when ready'),
        key: 'shift+m',
        reason:
          rv.kind === 'authored' && workOf(item)
            ? null
            : t('Merge when ready: only for my PRs with a work item'),
        run: () => mergeWhenReady(item),
      },
    ];
  }

  function rowMenu(e: MouseEvent, l: Extract<Line, { type: 'row' }>): void {
    e.preventDefault();
    selId = l.id;
    more = { x: e.clientX, y: e.clientY, actions: rowActions(l.s.item) };
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
</script>

<!-- One list query per focus (not a get per row): it also brings back PRs updated after my review. -->
<svelte:window onfocus={() => void reviews.load(scope, 'review_requested', true)} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane"
  data-testid="reviews-pane"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={t('Reviews')}
  {onkeydown}
  use:onActions={menuKey}
>
  <!-- k-toolbar: its selects keep their label for screen readers only. -->
  <header class="bar k-toolbar">
    <span class="title">{t('Reviews')}</span>
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
    {#if Number.isFinite(fetchedAt)}
      <!-- shortcut: the age is computed on render (no timers), it ages with the next re-render. -->
      <span class="asof" data-testid="reviews-asof"
        >{t('Updated {ago}', { ago: relativeTime(fetchedAt) })}</span
      >
    {/if}
  </header>
  <FilterBar
    bind:value={filter}
    bind:input={filterInput}
    active={activeFilters}
    placeholder={t('Filter title, repo, number, author, branch, ticket (/)')}
    aria-label={t('Filter reviews')}
    onkeydown={filterKeys}
  />
  <LoadBar active={!noData && shown.some((l) => l.loading)} label={t('Loading reviews')} />

  {#if noData && !firstError}
    <Loading label={t('Loading reviews')} />
  {:else if noData && firstError}
    <ErrorState error={firstError} title={t('Could not load reviews')} onretry={refresh}>
      {#snippet actions()}
        <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
          {isAuthError(firstError) ? t('Re-authenticate') : t('Open account settings')}
        </Button>
      {/snippet}
    </ErrorState>
  {:else}
    <StateBanner
      stale={shown.some((l) => l.stale)}
      fetchedAt={Number.isFinite(fetchedAt) ? fetchedAt : undefined}
      error={firstError}
      errors={accountErrors}
      onretry={refresh}
    />
    {#if teamOffered && noTeamReach}
      <div class="nosource" data-testid="reviews-no-team">
        {t('No ticket source feeds a project with a repository yet')} ·
        <button
          type="button"
          class="k-link"
          onclick={() =>
            ui.openSheet('tracker.source_picker', {
              projectId: scope.kind === 'project' ? scope.id : undefined,
            })}>{t('Add a ticket source')}</button
        >
      </div>
    {/if}
    {#if itemRows.length === 0 && activeFilters.length > 0}
      <EmptyState icon="search" title={noMatch(activeFilters)}>
        {#snippet actions()}<Button onclick={() => clearAll(activeFilters)}>{t('Clear all')}</Button
          >{/snippet}
      </EmptyState>
    {:else}
      <div class="body">
        <div class="main">
          <div class="cols grid" aria-hidden="true">
            <span></span><span></span>
            <span class="legend"
              >{#each AREAS as a (a)}<span><i class={a}></i>{t(AREA_LABEL[a])}</span>{/each}</span
            >
            <span class="right">{t('Open')}</span><span></span>
          </div>
          <div class="list">
            <VirtualList
              bind:this={vlist}
              items={lines}
              itemHeight={ROW_HEIGHT}
              measure
              key={(l) => l.id}
              label={t('Pull requests')}
              sticky={(l) => l.type === 'section'}
            >
              {#snippet row(l)}
                {#if l.type === 'section'}
                  {@const quiet = SECTIONS.find((s) => s.id === l.section)?.quiet}
                  <h2 class="section" class:quiet data-section={l.section}>
                    <button
                      type="button"
                      tabindex="-1"
                      aria-expanded={l.open}
                      onclick={() => toggleSection(l.section)}
                    >
                      <span class="chevron"><Icon name="chevron-right" size={12} /></span>
                      {t(l.label)}
                      <span class="count">{l.count}</span>
                    </button>
                  </h2>
                {:else if l.type === 'none'}
                  <p class="none">{l.text}</p>
                {:else}
                  {@const rv = l.s.item.review}
                  {@const key = reviewKey(rv.ref)}
                  {@const sel = cur === l}
                  {@const mine = l.section === 'mine'}
                  {@const map = changeMap(rv, rv.files)}
                  {@const days = openDays(rv)}
                  {@const failing = rv.ci === 'failure' || rv.ci === 'error'}
                  <button
                    type="button"
                    tabindex="-1"
                    class="row grid"
                    class:selected={sel}
                    aria-current={sel ? 'true' : undefined}
                    data-key={key}
                    onclick={() => (selId = l.id)}
                    ondblclick={() => openDetail(l.s.item)}
                    oncontextmenu={(e) => rowMenu(e, l)}
                  >
                    <span class="num"
                      >{#if inflight.has(key)}<Spinner size={10} />{/if}#{rv.ref.number}</span
                    >
                    <span class="what" style:padding-left="{Math.max(0, l.s.depth - 1) * 12}px">
                      {#if l.s.depth > 0}<span class="elbow" aria-hidden="true">└</span>{/if}
                      <span class="txt">
                        <span class="ttl">{rv.title}</span>
                        <span class="sub">
                          {#if failing}<span class="bad">{t('Checks failed')}</span
                            >{:else if rv.ci === 'pending'}<span>{t('Checks running')}</span>{/if}
                          <span class="state">{rowState(l.s)}</span>
                          {#if !mine}<span>{rv.author.name}</span>{/if}
                          {#if reviews.isNew(rv.ref)}<Badge tone="accent">{t('new')}</Badge>{/if}
                          {#if reviewPhase(rv) === 'updated'}<span class="warn"
                              >{t('Updated since your review')}</span
                            >{/if}
                          {#if workOf(l.s.item)?.auto_finish}<Badge tone="accent">{t('armed')}</Badge>{/if}
                          <TagChips kind="prs" itemKey={key} />
                        </span>
                      </span>
                    </span>
                    <span class="mapcell"
                      >{#if map}<ChangeMap {map} />{/if}</span
                    >
                    <span class="age" class:stale={!mine && days > STALE_DAYS}
                      >{days === 0 ? t('today') : t('{n} d', { n: days })}</span
                    >
                    <span class="acts"
                      >{#if sel}<RowButton
                          label={t('More actions')}
                          icon="ellipsis"
                          onclick={(e) => (more = { ...below(e), actions: rowActions(l.s.item) })}
                        />{/if}</span
                    >
                  </button>
                {/if}
              {/snippet}
            </VirtualList>
          </div>
        </div>
        <aside class="side" bind:this={side} aria-label={t('Review detail')}>
          {#if cur}
            <!-- The PR's project runs its actions (fix, review locally); a PR no project binds uses the active one. -->
            {@const pid = home ?? cur.s.item.project_ids[0] ?? projects.activeId}
            {#if pid}
              <!-- Keyed: the "PR changed" banner and an open dialog belong to one PR. -->
              {#key cur.id}
                <!-- A crash in one PR's detail stays in the side panel; the list keeps working. -->
                <svelte:boundary onerror={(e) => console.error(`[kelta] review detail ${cur.id} crashed`, e)}>
                  <ReviewDetailPane
                    projectId={pid}
                    {tabId}
                    {paneId}
                    content={{ kind: 'review_detail', review: cur.s.item.review.ref }}
                    visible
                    focused={false}
                    embedded
                  />
                  {#snippet failed(error)}
                    <ErrorState
                      error={error instanceof Error ? error : String(error)}
                      title={t('Could not show this pull request')}
                    />
                  {/snippet}
                </svelte:boundary>
              {/key}
            {/if}
          {:else}
            <p class="k-split-empty">{t('Select a pull request to see it here.')}</p>
          {/if}
        </aside>
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
{#if more}
  <ActionMenu
    {...more}
    onclose={() => {
      more = null;
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
  .pane {
    container: reviews / inline-size;
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

  .asof {
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .spacer {
    flex: 1;
  }

  .nosource {
    padding: var(--k-space-2) var(--k-space-4);
    border-bottom: 1px solid var(--k-border);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  /* The list, and the selected PR's brief beside it (as Now, #399): 340px, 300px below 1180px, under
     the list below 900px. */
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

  /* number, title and state, the change map, the age, the row's menu: one grid for header and rows. */
  .grid {
    display: grid;
    grid-template-columns: 60px minmax(0, 1fr) 170px 48px 24px;
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

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 2px var(--k-space-2);
  }

  .legend > span {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
  }

  .legend i {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }

  .legend .code {
    background: var(--k-fg-muted);
  }

  .legend .tests {
    background: var(--k-info);
  }

  .legend .config {
    background: var(--k-warn);
  }

  .legend .other {
    background: var(--k-border-strong);
  }

  .right {
    text-align: right;
  }

  .section {
    margin: 0;
    height: 100%;
    background: var(--k-bg);
  }

  .section button {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    width: 100%;
    padding: var(--k-space-4) var(--k-space-4) var(--k-space-1);
    border: 0;
    background: none;
    font: inherit;
    font-size: var(--k-font-size-sm);
    font-weight: 600;
    text-align: left;
    color: var(--k-fg-strong);
    cursor: pointer;
  }

  .section.quiet button {
    color: var(--k-fg-muted);
  }

  .count {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
  }

  .chevron {
    display: inline-flex;
  }

  .chevron :global(svg) {
    transition: rotate var(--k-duration-fast) var(--k-ease);
  }

  .section [aria-expanded='true'] .chevron :global(svg) {
    rotate: 90deg;
  }

  .none {
    margin: 0;
    padding: 0 var(--k-space-4) var(--k-space-1) calc(var(--k-space-4) + 60px + var(--k-space-4));
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-subtle);
  }

  .row {
    width: 100%;
    min-height: 56px;
    padding-block: var(--k-space-2);
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--k-font-size);
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--k-bg-hover);
  }

  /* Subtle and muted text on the selected row take their -sel steps (as Now, #399). */
  .selected {
    background: var(--k-bg-selected-row);
    --k-fg-subtle: var(--k-fg-subtle-sel);
    --k-fg-muted: var(--k-fg-muted-sel);
  }

  .num {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .what {
    display: flex;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .elbow {
    flex: none;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-subtle);
  }

  .txt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  /* Titles wrap to two lines instead of one cut line (as Now, #399). */
  .ttl {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    color: var(--k-fg-strong);
  }

  .sub {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-subtle);
  }

  .sub > span {
    flex: none;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sub .state {
    flex: 0 1 auto;
    color: var(--k-fg-muted);
  }

  .sub .bad {
    color: var(--k-danger);
  }

  .sub .warn {
    color: var(--k-warn);
  }

  .mapcell {
    min-width: 0;
  }

  .age {
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    text-align: right;
    color: var(--k-fg-subtle);
  }

  .age.stale {
    font-weight: 600;
    color: var(--k-warn);
  }

  .acts {
    display: flex;
    justify-content: flex-end;
  }

  @container reviews (max-width: 1115px) {
    .body {
      grid-template-columns: minmax(0, 1fr) 300px;
    }

    .grid {
      grid-template-columns: 60px minmax(0, 1fr) 120px 48px 24px;
    }
  }

  /* Narrow: the brief stacks under the list, which keeps at least half the height. Pane widths: the
     window minus the 64px compact rail (835 = a 900px window, 1115 = 1180px). */
  @container reviews (max-width: 835px) {
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

  @container reviews (max-width: 699px) {
    .cols {
      display: none;
    }

    .grid {
      grid-template-columns: 52px minmax(0, 1fr) 40px 24px;
      row-gap: var(--k-space-1);
      padding-inline: var(--k-space-3);
    }

    .row .mapcell {
      grid-column: 2 / 5;
      grid-row: 2;
    }

    .none {
      padding-left: var(--k-space-3);
    }
  }
</style>
