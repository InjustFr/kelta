<script lang="ts">
  // The Story panel's Pull request section (#426): number, title and draft or ready; the checks (failed
  // first), the reviewers and who the PR waits on, the unresolved threads; the buttons its phase offers.
  // Reads the polled lists; the checks come from the PR detail and the thread excerpt from `work_feedback`.
  import { untrack } from 'svelte';

  import { t, tn } from '$lib/i18n/index.svelte';

  import type { FeedbackThread, WorkItem } from '$lib/gen';
  import { workFeedback } from '$lib/ipc/commands';
  import { reviews, settings } from '$lib/stores';
  import { reviewKey } from '$lib/stores/reducers';
  import { Button, Lamp } from '$lib/ui';

  import { checksFirst, prWait, REVIEWER_STATE } from '../../reviews/pullRequest';
  import { blockedReason, runWorkAction } from '../actions';
  import { ciGlyph, decisionInfo } from '../common';
  import { phaseNow, prOf } from '../live';
  import type { WorkActionId } from '../phase';
  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  const pr = $derived(prOf(item));
  const num = $derived(pr?.ref.number ?? item.pr_url?.match(/\/(\d+)\/?$/)?.[1] ?? null);
  const word = $derived(
    item.state.kind === 'merged'
      ? t('merged')
      : item.state.kind === 'pr_closed'
        ? t('closed')
        : pr?.draft
          ? t('draft')
          : t('open'),
  );
  const live = $derived(item.state.kind !== 'merged' && item.state.kind !== 'pr_closed');
  // A host that lists no reviewers still says approved / changes requested.
  const decision = $derived(pr && pr.reviewers.length === 0 ? decisionInfo(pr.decision) : null);
  const threads = $derived(pr?.unresolved_threads ?? 0);
  const wait = $derived(pr ? prWait(pr, settings.value()?.reviews.sla_hours ?? 24) : null);
  const phase = $derived(phaseNow(item));

  // The full check list is in the PR detail; until it is there, the failed names of the polled list.
  const prKey = $derived(pr ? `${reviewKey(pr.ref)}@${pr.head_sha}` : null);
  $effect(() => {
    if (prKey) untrack(() => pr && void reviews.loadDetail(pr.ref));
  });
  const checks = $derived(
    checksFirst(
      (pr && reviews.details[reviewKey(pr.ref)]?.data?.checks) ||
        (pr?.failed_check_names ?? []).map((name) => ({ name, state: 'failure' as const, url: null })),
    ),
  );

  let first = $state<FeedbackThread | null>(null);
  $effect(() => {
    const id = item.id;
    first = null;
    if (threads === 0) return;
    let current = true;
    workFeedback({ id })
      .then((fb) => current && (first = fb.threads[0] ?? null))
      .catch(() => {}); // the excerpt is optional
    return () => (current = false);
  });

  const buttons = $derived.by(() => {
    if (!pr && !item.pr_url) return [];
    const out: { id: WorkActionId; label: string }[] = [];
    if (pr?.ci === 'failure' || pr?.ci === 'error')
      out.push({ id: 'fix_checks', label: t('Send failures to Claude') });
    if (threads > 0 || pr?.decision === 'changes_requested')
      out.push({ id: 'fix', label: t('Fix with Claude') });
    if (live)
      out.push(
        pr?.draft
          ? { id: 'set_draft', label: t('Mark ready') }
          : { id: 'merge', label: item.auto_finish ? t('Disarm merge when ready') : t('Merge when ready') },
      );
    out.push({ id: 'open_pr', label: t('Open PR') });
    return out;
  });
</script>

<Section
  title={t('Pull request')}
  summary={item.pr_url || pr ? `${num ? `#${num} ` : ''}${word}` : t('No pull request')}
  {open}
  testid="story-pr"
>
  {#if item.pr_url || pr}
    <div class="head">
      <span class="num">#{num}</span>
      <span class="title">{pr?.title ?? ''}</span>
      <span class="state">{live ? (pr?.draft ? t('Draft') : t('Ready for review')) : word}</span>
    </div>
    {#if checks.length > 0}
      <ul class="plain" aria-label={t('Checks')} data-testid="story-pr-checks">
        {#each checks as c, i (i)}
          {@const g = ciGlyph(c.state)}
          <li><Lamp level={g.lamp} title={g.label} />{c.name}</li>
        {/each}
      </ul>
    {/if}
    {#if pr && (pr.reviewers.length > 0 || decision || wait)}
      <ul class="plain" data-testid="story-pr-reviewers">
        {#each pr.reviewers as r (r.user.id)}
          <li>
            <span class="who">{r.user.name}</span>
            <span class="st {r.state ?? 'pending'}">{t(REVIEWER_STATE[r.state ?? 'pending'] ?? r.state)}</span
            >
          </li>
        {/each}
        {#if decision}<li class="st {pr.decision}">{decision.label}</li>{/if}
        {#if wait}<li class="wait" class:late={wait.late} title={wait.title}>{wait.label}</li>{/if}
      </ul>
    {/if}
    {#if threads > 0}
      <p class="threads" data-testid="story-pr-threads">
        {tn('{n} unresolved thread', '{n} unresolved threads', threads)}
      </p>
      {#if first}<blockquote class="excerpt">{first.body_md.trim()}</blockquote>{/if}
    {/if}
    {#if item.auto_finish}<p class="muted">{t('Merge when ready')}: {t('armed')}</p>{/if}
    <div class="buttons">
      {#each buttons as b (b.id)}
        {@const why = blockedReason(b.id, item, phase)}
        <Button
          size="sm"
          variant={phase.primary === b.id ? 'primary' : 'secondary'}
          disabled={why !== null}
          title={why ?? undefined}
          data-testid="story-pr-{b.id}"
          onclick={() => void runWorkAction(b.id, item)}>{b.label}</Button
        >
      {/each}
    </div>
  {:else}
    <p class="muted">{t('Ship pushes the branch and opens a draft PR.')}</p>
    <div class="buttons">
      <Button
        size="sm"
        variant="primary"
        disabled={blockedReason('ship', item, phase) !== null}
        data-testid="story-pr-ship"
        onclick={() => void runWorkAction('ship', item)}>{t('Ship')}</Button
      >
    </div>
  {/if}
</Section>

<style>
  .head {
    display: flex;
    gap: var(--k-space-2);
    align-items: baseline;
  }

  .num {
    font-family: var(--k-font-mono);
    font-weight: 600;
  }

  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state,
  .muted {
    color: var(--k-fg-muted);
  }

  .plain {
    padding: 0;
    margin: var(--k-space-2) 0;
    list-style: none;
  }

  .plain li {
    display: flex;
    gap: var(--k-space-2);
    align-items: center;
    padding: 2px 0;
  }

  .who {
    flex: 1;
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

  .wait {
    color: var(--k-fg-muted);
  }

  .wait.late {
    color: var(--k-warn);
  }

  .threads {
    margin: var(--k-space-2) 0 0;
  }

  .excerpt {
    display: -webkit-box;
    margin: var(--k-space-1) 0 0;
    padding-left: var(--k-space-2);
    overflow: hidden;
    border-left: 2px solid var(--k-border);
    color: var(--k-fg-muted);
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2);
    margin-top: var(--k-space-3);
  }
</style>
