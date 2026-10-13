<script lang="ts">
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { openExternal, reviewApprove, reviewComment, reviewRequestChanges } from '$lib/ipc/commands';
  import { isIpcError } from '$lib/ipc/transport';
  import { reviews, toasts } from '$lib/stores';
  import { reviewKey } from '$lib/stores/reducers';
  import { Badge, Button, EmptyState, ErrorState, HtmlContent, Icon, Lamp } from '$lib/ui';

  import { reviewCtx } from '../plugins/actions';
  import PluginActions from '../plugins/PluginActions.svelte';
  import { ciGlyph, isAuthError, myStateInfo } from '../work/common';
  import Brief, { type BriefAction } from '../work/shared/Brief.svelte';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { reviewLocally } from '../work/startWork';
  import { configWorkflows, pickPrWorkflow } from '../tickets/workflow.svelte';
  import { draftBlocked, draftLabel, toggleDraft } from './draft';
  import { beforeMerging } from './beforeMerging';
  import ChangeMap from './ChangeMap.svelte';
  import { AREA_LABEL, changeMap, filesByArea } from './changemap';
  import LinkedTicket from './LinkedTicket.svelte';
  import { openDays, reviewState } from './sections';
  import ReviewTextDialog from './ReviewTextDialog.svelte';
  import TagChips from '../tags/TagChips.svelte';
  import TagMenu from '../tags/TagMenu.svelte';

  /** `embedded`: the Reviews split view's column; the list owns `Esc` and the focus. */
  let {
    projectId,
    content,
    embedded = false,
  }: PaneProps<'review_detail'> & { embedded?: boolean } = $props();

  const ref = $derived(content.review);
  const slot = $derived(reviews.details[reviewKey(ref)]);
  const detail = $derived(slot?.data ?? null);
  const rv = $derived(detail?.review ?? null);
  /** The commit shown on screen: the only value `Approve` ever sends. */
  const shownSha = $derived(rv?.head_sha ?? null);

  /** Line comments waiting in my pending review; a / c / m publish them with the decision. */
  const pendingCount = $derived(detail?.pending_comments ?? 0);

  let changed = $state(false);
  let approving = $state(false);
  let dialog = $state<'comment' | 'changes' | null>(null);
  let root = $state<HTMLDivElement>();
  let tagMenu = $state<{ x: number; y: number } | null>(null);
  /** The brief shows every action (`.`). */
  let more = $state(false);

  /** The Tags menu opens under its button, or the brief's top when it waits behind "more". */
  function openTags(): void {
    const r = (root?.querySelector('[data-key="t"]') ?? root)?.getBoundingClientRect();
    tagMenu = { x: r?.left ?? 40, y: (r?.top ?? 40) + Math.min(r?.height ?? 0, 32) + 2 };
  }

  $effect(() => {
    const r = ref;
    untrack(() => reviews.markSeen(r));
    if (!embedded) return void untrack(() => reviews.loadDetail(r));
    // Walking the list with j/k: only the row that stays 150 ms loads (the `{#key}` teardown cancels).
    const t = setTimeout(() => void reviews.loadDetail(r), 150);
    return () => clearTimeout(t);
  });

  const REVIEWER_STATE: Record<string, string> = {
    approved: msg('approved'),
    changes_requested: msg('changes requested'),
    commented: msg('commented'),
    pending: msg('pending'),
    dismissed: msg('dismissed'),
  };
  function refresh(): void {
    changed = false;
    void reviews.loadDetail(ref);
  }

  async function approve(): Promise<void> {
    const sha = shownSha;
    if (!sha || approving) return;
    approving = true;
    try {
      await reviewApprove({ review: ref, head_sha: sha });
      toasts.info(t('Approved {pr} at {sha}', { pr: `${ref.repo}#${ref.number}`, sha: sha.slice(0, 7) }));
      changed = false;
      void reviews.loadDetail(ref);
    } catch (err) {
      if (isIpcError(err, 'conflict')) {
        changed = true;
        toasts.warn(t('PR changed, refresh'));
      } else {
        toasts.error(err, t('Approve'));
      }
    } finally {
      approving = false;
    }
  }

  async function send(kind: 'comment' | 'changes', text: string): Promise<void> {
    try {
      if (kind === 'comment') await reviewComment({ review: ref, body: text });
      else await reviewRequestChanges({ review: ref, body: text });
      toasts.info(kind === 'comment' ? t('Comment posted') : t('Changes requested'));
      void reviews.loadDetail(ref);
    } catch (err) {
      toasts.error(err, kind === 'comment' ? t('Comment') : t('Request changes'));
      throw err;
    }
  }

  function browse(): void {
    if (rv) openExternal({ url: rv.url }).catch((err) => toasts.error(err, t('Open in browser')));
  }

  /** My own PR: fixed through its work item, never approved here (FLOW §4.2). */
  const own = $derived(rv?.kind === 'authored');

  /** After the primary (Review locally, Go to work item on mine), in likely order. */
  const briefActions = $derived.by((): BriefAction[] => {
    if (!rv) return [];
    const r = rv;
    const tail: BriefAction[] = [
      { key: 't', label: t('Tags'), run: openTags },
      ...(configWorkflows('pr').length > 0
        ? [{ key: 'w', label: t('Run workflow'), run: () => pickPrWorkflow(ref, projectId) }]
        : []),
      { key: 'shift+r', label: t('Refresh'), run: refresh },
    ];
    if (own)
      return [
        { key: 'f', label: t('Fix with Claude'), run: fixWithClaude },
        ...(draftBlocked(r) ? [] : [{ key: 'd', label: draftLabel(r), run: () => toggleDraft(r) }]),
        { key: 'o', label: t('Open in browser'), run: browse },
        ...tail,
      ];
    return [
      { key: 'a', label: t('Approve'), run: approve },
      { key: 'm', label: t('Comment'), run: () => (dialog = 'comment') },
      { key: 'c', label: t('Request changes'), run: () => (dialog = 'changes') },
      { key: 'o', label: t('Open in browser'), run: browse },
      ...tail,
    ];
  });

  function fixWithClaude(): void {
    void dispatch('work.fix_pr', { review: ref, project_id: projectId });
  }

  function onkeydown(e: KeyboardEvent): void {
    if ((e.target as HTMLElement).closest('input, textarea, select, [role="dialog"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (own && (e.key === 'f' || e.key === 'g' || e.key === 'd')) {
      e.preventDefault();
      if (e.key === 'f') fixWithClaude();
      else if (e.key === 'd') void (rv && toggleDraft(rv));
      else void reviewLocally(ref, projectId);
      return;
    }
    if (own && ['a', 'c', 'm'].includes(e.key)) return;
    switch (e.key) {
      case 'a':
        void approve();
        break;
      case 'c':
        dialog = 'changes';
        break;
      case 'm':
        dialog = 'comment';
        break;
      case 'o':
        browse();
        break;
      case 's':
        void reviewLocally(ref, projectId);
        break;
      case 'R':
        refresh();
        break;
      case 't':
        openTags();
        break;
      case 'w':
        pickPrWorkflow(ref, projectId);
        break;
      case '.':
        more = !more;
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  $effect(() => {
    if (!embedded && root && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
  });
</script>

<!-- Window focus re-reads the PR: my review state, its head and my pending comments (no polling). -->
<svelte:window onfocus={() => void reviews.loadDetail(ref)} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane k-detail"
  class:embedded
  data-testid="review-detail"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={t('Review {pr}', { pr: `${ref.repo}#${ref.number}` })}
  {onkeydown}
>
  {#if slot?.loading && !detail}
    <Loading label={t('Loading review')} />
  {:else if !detail && slot?.error}
    {#if slot.error.code === 'not_found'}
      <EmptyState
        icon="git-pull-request"
        title={t('This pull request is gone')}
        body={t('It may have been merged, closed or deleted.')}
      >
        {#snippet actions()}<Button onclick={refresh}>{t('Retry')}</Button>{/snippet}
      </EmptyState>
    {:else}
      <ErrorState error={slot.error} title={t('Could not load the review')} onretry={refresh}>
        {#snippet actions()}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
            {isAuthError(slot.error) ? t('Re-authenticate') : t('Open settings')}
          </Button>
        {/snippet}
      </ErrorState>
    {/if}
  {:else if detail && rv}
    {@const ci = ciGlyph(rv.ci)}
    {@const mine = myStateInfo(rv.my_state)}
    <StateBanner
      stale={slot?.stale ?? false}
      fetchedAt={slot?.fetchedAt}
      error={slot?.error}
      onretry={refresh}
    />
    {#if changed}
      <div class="changed" role="alert" data-testid="pr-changed">
        <Icon name="triangle-alert" size={14} />
        <span>{t('PR changed, refresh. The head moved since you opened it, nothing was approved.')}</span>
        <Button size="sm" onclick={refresh}>{t('Refresh')}</Button>
      </div>
    {/if}
    {@const status = reviewState({
      reviewers: detail.reviewers.filter((r) => r.state && r.state !== 'pending'),
      waiting_on: detail.reviewers.filter((r) => !r.state || r.state === 'pending').map((r) => r.user.name),
      decision: rv.decision,
    })}
    {@const map = changeMap(rv, detail.files)}
    {@const body = beforeMerging(detail.body_html)}
    {@const days = openDays(rv)}
    <Brief
      id="{ref.repo}#{ref.number}"
      title={rv.title}
      where={[
        rv.author.name,
        days === 0 ? t('Open since today') : tn('Open {n} day', 'Open {n} days', days),
        ...(rv.ci === 'none' ? [] : [ci.label]),
        ...(rv.mergeable === false ? [t('conflicts')] : []),
      ].join(' · ')}
      primary={own
        ? { key: 'g', label: t('Go to work item'), run: () => reviewLocally(ref, projectId) }
        : { key: 's', label: t('Review locally'), run: () => reviewLocally(ref, projectId) }}
      actions={briefActions}
      bind:more
      testid="review-brief"
    >
      {#snippet sub()}
        <p class="branches">
          <code>{rv.source_branch}</code> → <code>{rv.target_branch}</code>
          <code data-testid="head-sha" title={rv.head_sha}>{rv.head_sha.slice(0, 7)}</code>
          {#if rv.draft}<Badge>{t('draft')}</Badge>{/if}
          {#each rv.linked_tickets as key (key)}<LinkedTicket
              ticketKey={key}
              {projectId}
              repo={rv.ref.repo}
            />{/each}
          <TagChips kind="prs" itemKey={reviewKey(ref)} />
        </p>
      {/snippet}
      <PluginActions kind="review" size="md" ctx={reviewCtx(rv.ref, projectId)} />
      <section class="callout" aria-label={t('Review status')} data-testid="review-status">
        <b>{t('Review status')}</b>
        <p>{status}</p>
        {#if mine}<p><Badge tone={mine.tone}>{mine.label}</Badge></p>{/if}
        {#if detail.reviewers.length > 0}
          <ul class="reviewers">
            {#each detail.reviewers as r, i (i)}
              <li>
                <span>{r.user.name}</span>
                <span class="st {r.state ?? 'pending'}"
                  >{t(REVIEWER_STATE[r.state ?? 'pending'] ?? r.state)}</span
                >
              </li>
            {/each}
          </ul>
        {/if}
        {#if detail.pending_comments > 0}
          <p class="pending" data-testid="pending-comments">
            {tn('{n} pending comment', '{n} pending comments', detail.pending_comments)}
          </p>
        {/if}
      </section>
      {#if body.before}
        <section class="callout heads" aria-label={t('Before merging')} data-testid="before-merging">
          <b>{t('Before merging, from the description')}</b>
          <HtmlContent html={body.before} onerror={(e) => toasts.error(e, t('Open link'))} />
        </section>
      {/if}
      {#if map}<ChangeMap {map} big />{/if}
      <section aria-label={t('Description')}>
        <details open>
          <summary>{t('Description')}</summary>
          {#if body.rest.trim() === ''}
            <p class="muted">{t('No description.')}</p>
          {:else}
            <HtmlContent html={body.rest} onerror={(e) => toasts.error(e, t('Open link'))} />
          {/if}
        </details>
      </section>
      {#if detail.files.length > 0}
        {@const big = Math.max(...detail.files.map((f) => f.additions + f.deletions), 1)}
        <section aria-label={t('Files')}>
          <details open>
            <summary
              >{t('Files')}
              <small>{tn('{n}, biggest first', '{n}, biggest first', detail.files.length)}</small></summary
            >
            {#each filesByArea(detail.files) as g (g.area)}
              <h3 class="area">
                <i class={g.area}></i>{t(AREA_LABEL[g.area])} <small>{g.files.length}</small>
              </h3>
              <ul class="files">
                {#each g.files as f, i (i)}
                  {@const cut = f.path.lastIndexOf('/') + 1}
                  <li>
                    <span class="path" title={f.path}
                      ><span><span class="dir">{f.path.slice(0, cut)}</span>{f.path.slice(cut)}</span></span
                    >
                    <span class="fbar" aria-hidden="true"
                      ><i class="a" style:width="{(f.additions / big) * 100}%"></i><i
                        class="d"
                        style:width="{(f.deletions / big) * 100}%"
                      ></i></span
                    >
                    <span class="k-mono k-num delta"
                      ><span class="add">+{f.additions}</span> <span class="del">−{f.deletions}</span></span
                    >
                  </li>
                {/each}
              </ul>
            {/each}
          </details>
        </section>
      {/if}
      {#if detail.checks.length > 0}
        <section aria-label={t('Checks')}>
          <details open={rv.ci === 'failure' || rv.ci === 'error'}>
            <summary>{t('Checks')} <small>{detail.checks.length}</small></summary>
            <ul class="plain">
              <!-- Index keys: a host can report two checks with one name (check run + commit status). -->
              {#each detail.checks as c, i (i)}
                {@const g = ciGlyph(c.state)}
                <li>
                  <span class="k-row-lamp"><Lamp level={g.lamp} title={g.label} /></span>
                  {#if c.url}
                    {@const url = c.url}
                    <button
                      type="button"
                      class="link"
                      onclick={() => openExternal({ url }).catch((err) => toasts.error(err, t('Open check')))}
                    >
                      {c.name}
                    </button>
                  {:else}
                    {c.name}
                  {/if}
                </li>
              {/each}
            </ul>
          </details>
        </section>
      {/if}
    </Brief>
  {:else}
    <Loading label={t('Loading review')} />
  {/if}
</div>

{#if dialog === 'comment'}
  <ReviewTextDialog
    title={t('Comment')}
    label={t('Comment (Markdown)')}
    submitLabel={t('Comment')}
    required={!pendingCount}
    onsubmit={(text) => send('comment', text)}
    onclose={() => {
      dialog = null;
      root?.focus();
    }}
  />
{:else if dialog === 'changes'}
  <ReviewTextDialog
    title={t('Request changes')}
    label={t('What should change? (Markdown)')}
    submitLabel={t('Request changes')}
    required={!pendingCount}
    danger
    onsubmit={(text) => send('changes', text)}
    onclose={() => {
      dialog = null;
      root?.focus();
    }}
  />
{/if}
{#if tagMenu}
  <TagMenu
    kind="prs"
    itemKey={reviewKey(ref)}
    x={tagMenu.x}
    y={tagMenu.y}
    onclose={() => {
      tagMenu = null;
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
    overflow-y: auto;
    outline: none;
    background: var(--k-bg-elev);
    color: var(--k-fg);
  }

  /* The split view's column paints the surface and scrolls (base.css .k-split-side). */
  .embedded {
    height: auto;
    overflow: visible;
    background: none;
  }

  /* Split view: list and detail share one pane, so mark which side has the keys (DESIGN §7). */
  .embedded:focus-visible {
    box-shadow: inset 2px 0 0 var(--k-focus);
  }

  .changed {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-4);
    background: var(--k-bezel-raised);
    color: var(--k-warn);
  }

  .changed span {
    flex: 1;
  }

  .pending {
    color: var(--k-warn);
  }

  .branches {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-1) var(--k-space-2);
    margin: var(--k-space-1) 0 0;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-subtle);
    overflow-wrap: anywhere;
  }

  .callout {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    padding: var(--k-space-2) var(--k-space-3);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg);
    font-size: var(--k-font-size-sm);
  }

  .callout b {
    font-size: var(--k-font-size-xs);
    font-weight: 600;
    color: var(--k-fg-subtle);
  }

  .callout p {
    margin: 0;
  }

  /* Before merging: the description's deploy steps, on the warning surface so they are not missed. */
  .heads {
    background: color-mix(in srgb, var(--k-warn) 14%, var(--k-bg));
  }

  .heads b {
    color: var(--k-warn);
  }

  .reviewers {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .reviewers li {
    display: flex;
    justify-content: space-between;
    gap: var(--k-space-3);
  }

  .st {
    color: var(--k-fg-subtle);
  }

  .st.approved {
    color: var(--k-ok);
  }

  .st.changes_requested {
    color: var(--k-danger);
  }

  details {
    border-top: 1px solid var(--k-border);
  }

  summary {
    padding: var(--k-space-2) 0;
    font-weight: 600;
    cursor: pointer;
  }

  summary small,
  .area small {
    font-weight: 400;
    color: var(--k-fg-subtle);
  }

  .area {
    display: flex;
    align-items: center;
    gap: var(--k-space-1);
    margin: var(--k-space-2) 0 var(--k-space-1);
    font-size: var(--k-font-size-xs);
    font-weight: 600;
    color: var(--k-fg-muted);
  }

  .area i {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }

  .area .code {
    background: var(--k-fg-muted);
  }

  .area .tests {
    background: var(--k-info);
  }

  .area .config {
    background: var(--k-warn);
  }

  .area .other {
    background: var(--k-border-strong);
  }

  .files {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--k-font-size-sm);
  }

  .files li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 56px auto;
    align-items: center;
    gap: var(--k-space-2);
  }

  /* Long paths keep their file name: the directory is cut at its start. */
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .path > span {
    direction: ltr;
    unicode-bidi: plaintext;
  }

  .dir {
    color: var(--k-fg-subtle);
  }

  .fbar {
    display: flex;
    gap: 1px;
    height: 6px;
  }

  .fbar i {
    display: block;
    height: 100%;
    border-radius: 1px;
  }

  .fbar .a {
    background: var(--k-ok);
  }

  .fbar .d {
    background: var(--k-danger);
  }

  .delta {
    font-size: var(--k-font-size-xs);
    text-align: right;
  }

  .plain {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-2);
    padding: 0;
    list-style: none;
  }

  .plain li {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .add {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }

  .muted {
    color: var(--k-fg-subtle);
  }

  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-accent);
    cursor: pointer;
  }
</style>
