<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { openExternal, reviewApprove, reviewComment, reviewRequestChanges } from '$lib/ipc/commands';
  import { isIpcError } from '$lib/ipc/transport';
  import { reviews, toasts } from '$lib/stores';
  import { reviewKey } from '$lib/stores/reducers';
  import { Badge, Button, EmptyState, ErrorState, HtmlContent, Icon, Lamp } from '$lib/ui';

  import { ciGlyph, decisionInfo, isAuthError, myStateInfo } from '../work/common';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { reviewLocally } from '../work/startWork';
  import LinkedTicket from './LinkedTicket.svelte';
  import ReviewTextDialog from './ReviewTextDialog.svelte';

  let { projectId, content }: PaneProps<'review_detail'> = $props();

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

  $effect(() => {
    const r = ref;
    untrack(() => {
      void reviews.loadDetail(r);
      reviews.markSeen(r);
    });
  });

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
      toasts.info(`Approved ${ref.repo}#${ref.number} at ${sha.slice(0, 7)}`);
      changed = false;
      void reviews.loadDetail(ref);
    } catch (err) {
      if (isIpcError(err, 'conflict')) {
        changed = true;
        toasts.warn('PR changed, refresh');
      } else {
        toasts.error(err, 'Approve');
      }
    } finally {
      approving = false;
    }
  }

  async function send(kind: 'comment' | 'changes', text: string): Promise<void> {
    try {
      if (kind === 'comment') await reviewComment({ review: ref, body: text });
      else await reviewRequestChanges({ review: ref, body: text });
      toasts.info(kind === 'comment' ? 'Comment posted' : 'Changes requested');
      void reviews.loadDetail(ref);
    } catch (err) {
      toasts.error(err, kind === 'comment' ? 'Comment' : 'Request changes');
      throw err;
    }
  }

  function browse(): void {
    if (rv) openExternal({ url: rv.url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  /** My own PR: fixed through its work item, never approved here (FLOW §4.2). */
  const own = $derived(rv?.kind === 'authored');

  function fixWithClaude(): void {
    void dispatch('work.fix_pr', { review: ref, project_id: projectId });
  }

  function onkeydown(e: KeyboardEvent): void {
    if ((e.target as HTMLElement).closest('input, textarea, select, [role="dialog"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (own && (e.key === 'f' || e.key === 'g')) {
      e.preventDefault();
      if (e.key === 'f') fixWithClaude();
      else void reviewLocally(ref, projectId);
      return;
    }
    if (own && ['A', 'c', 'x'].includes(e.key)) return;
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
      default:
        return;
    }
    e.preventDefault();
  }

  $effect(() => {
    if (root && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
  });
</script>

<!-- Window focus re-reads the PR: my review state, its head and my pending comments (no polling). -->
<svelte:window onfocus={() => void reviews.loadDetail(ref)} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane k-detail"
  data-testid="review-detail"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={`Review ${ref.repo}#${ref.number}`}
  {onkeydown}
>
  {#if slot?.loading && !detail}
    <Loading label="Loading review" />
  {:else if !detail && slot?.error}
    {#if slot.error.code === 'not_found'}
      <EmptyState
        icon="git-pull-request"
        title="This pull request is gone"
        body="It may have been merged, closed or deleted."
      >
        {#snippet actions()}<Button onclick={refresh}>Retry</Button>{/snippet}
      </EmptyState>
    {:else}
      <ErrorState error={slot.error} title="Could not load the review" onretry={refresh}>
        {#snippet actions()}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
            {isAuthError(slot.error) ? 'Re-authenticate' : 'Open settings'}
          </Button>
        {/snippet}
      </ErrorState>
    {/if}
  {:else if detail && rv}
    {@const ci = ciGlyph(rv.ci)}
    {@const dec = decisionInfo(rv.decision)}
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
        <span>PR changed, refresh. The head moved since you opened it, nothing was approved.</span>
        <Button size="sm" onclick={refresh}>Refresh</Button>
      </div>
    {/if}
    <header class="head">
      <div class="line">
        <span class="key">{ref.repo}#{ref.number}</span>
        {#if rv.draft}<Badge>draft</Badge>{/if}
        <span class="check"><Lamp level={ci.lamp} title={ci.label} />{ci.label}</span>
        {#if dec}<Badge tone={dec.tone}>{dec.label}</Badge>{/if}
        {#if mine}<Badge tone={mine.tone}>{mine.label}</Badge>{/if}
        {#each rv.linked_tickets as t (t)}<LinkedTicket ticketKey={t} {projectId} repo={rv.ref.repo} />{/each}
      </div>
      <h1>{rv.title}</h1>
      <p class="branches"><code>{rv.source_branch}</code> into <code>{rv.target_branch}</code></p>
      <dl class="k-meta">
        <dt>Author</dt>
        <dd>{rv.author.name}</dd>
        <dt>Head</dt>
        <dd><code data-testid="head-sha" title={rv.head_sha}>{rv.head_sha.slice(0, 7)}</code></dd>
        {#if rv.additions !== null || rv.deletions !== null}<dt>Changes</dt>
          <dd class="k-mono k-num">
            <span class="add">+{rv.additions ?? 0}</span> <span class="del">−{rv.deletions ?? 0}</span>
          </dd>{/if}
        {#if rv.mergeable === false}<dt>Merge</dt>
          <dd><Badge tone="warn">conflicts</Badge></dd>{/if}
      </dl>
      <div class="line actions">
        {#if own}
          <Button variant="primary" icon="bot" onclick={fixWithClaude}>Fix with Claude</Button>
          <Button icon="git-branch" onclick={() => void reviewLocally(ref, projectId)}>Go to work item</Button
          >
          <Button variant="ghost" icon="external-link" onclick={browse}>Open in browser</Button>
        {:else}
          <Button variant="primary" icon="check" loading={approving} onclick={() => void approve()}
            >Approve</Button
          >
          <Button icon="message-square" onclick={() => (dialog = 'comment')}>Comment</Button>
          <Button variant="danger" onclick={() => (dialog = 'changes')}>Request changes</Button>
          {#if detail.pending_comments > 0}
            <span class="pending" data-testid="pending-comments">
              {detail.pending_comments} pending {detail.pending_comments === 1 ? 'comment' : 'comments'}
            </span>
          {/if}
          <Button variant="ghost" icon="external-link" onclick={browse}>Open in browser</Button>
          <Button variant="ghost" icon="git-branch" onclick={() => void reviewLocally(ref, projectId)}>
            Review locally
          </Button>
        {/if}
        <Button variant="ghost" icon="refresh-cw" onclick={refresh}>Refresh</Button>
      </div>
    </header>

    <div class="scroll">
      <section aria-label="Description">
        {#if detail.body_html.trim() === ''}
          <p class="muted">No description.</p>
        {:else}
          <HtmlContent html={detail.body_html} onerror={(e) => toasts.error(e, 'Open link')} />
        {/if}
      </section>

      <section aria-label="Reviewers">
        <h2>Reviewers</h2>
        {#if detail.reviewers.length === 0}
          <p class="muted">No reviewers.</p>
        {:else}
          <ul class="plain">
            {#each detail.reviewers as r (r.user.id)}
              <li>
                {r.user.name}
                {#if r.state}<Badge tone={myStateInfo(r.state)?.tone ?? 'neutral'}
                    >{r.state.replace('_', ' ')}</Badge
                  >{/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section aria-label="Checks">
        <h2>Checks</h2>
        {#if detail.checks.length === 0}
          <p class="muted">No checks reported.</p>
        {:else}
          <ul class="plain">
            {#each detail.checks as c (c.name)}
              {@const g = ciGlyph(c.state)}
              <li>
                <span class="k-row-lamp"><Lamp level={g.lamp} title={g.label} /></span>
                {#if c.url}
                  {@const url = c.url}
                  <button
                    type="button"
                    class="link"
                    onclick={() => openExternal({ url }).catch((err) => toasts.error(err, 'Open check'))}
                  >
                    {c.name}
                  </button>
                {:else}
                  {c.name}
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section aria-label="Files">
        <h2>Files <span class="muted k-num">{detail.files.length}</span></h2>
        <ul class="plain files">
          {#each detail.files as f (f.path)}
            <li>
              <code>{f.path}</code>
              <span class="k-mono k-num"
                ><span class="add">+{f.additions}</span> <span class="del">−{f.deletions}</span></span
              >
            </li>
          {/each}
        </ul>
      </section>
    </div>
  {:else}
    <Loading label="Loading review" />
  {/if}
</div>

{#if dialog === 'comment'}
  <ReviewTextDialog
    title="Comment"
    label="Comment (Markdown)"
    submitLabel="Comment"
    required={!pendingCount}
    onsubmit={(t) => send('comment', t)}
    onclose={() => {
      dialog = null;
      root?.focus();
    }}
  />
{:else if dialog === 'changes'}
  <ReviewTextDialog
    title="Request changes"
    label="What should change? (Markdown)"
    submitLabel="Request changes"
    required={!pendingCount}
    danger
    onsubmit={(t) => send('changes', t)}
    onclose={() => {
      dialog = null;
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
    background: var(--k-well);
    color: var(--k-fg);
  }

  .changed {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-5);
    background: var(--k-bezel-raised);
    color: var(--k-warn);
  }

  .pending {
    font-size: var(--k-font-size-sm);
    color: var(--k-warn);
  }

  .changed span {
    flex: 1;
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    padding: var(--k-space-5) var(--k-space-5) var(--k-space-4);
  }

  h1 {
    margin: 0;
    max-width: var(--k-measure);
    font-size: var(--k-font-size-xl);
    font-weight: var(--k-weight-strong);
    line-height: 1.28;
  }

  h2 {
    margin: var(--k-space-5) 0 var(--k-space-3);
    font-size: var(--k-font-size);
    font-weight: var(--k-weight-strong);
  }

  .line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--k-space-3);
  }

  .key {
    font-family: var(--k-font-mono);
    color: var(--k-fg-muted);
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .branches {
    margin: 0;
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 var(--k-space-5) var(--k-space-5);
  }

  .plain {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    max-width: var(--k-measure);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .plain li {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .files li {
    justify-content: space-between;
  }

  .add {
    color: var(--k-ok);
  }

  .del {
    color: var(--k-danger);
  }

  .muted {
    color: var(--k-fg-subtle);
    font-weight: 400;
  }

  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-accent);
    cursor: pointer;
  }
</style>
