<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { openExternal, workResume, workRetryStep } from '$lib/ipc/commands';
  import { sessions, toasts, work } from '$lib/stores';
  import { Badge, Button, EmptyState, Icon } from '$lib/ui';

  import { stepLabel } from './common';
  import WorkItemHeader from './WorkItemHeader.svelte';

  let { projectId, tabId, content }: PaneProps<'work_item'> = $props();

  const item = $derived(work.get(content.id));
  const itemSessions = $derived(
    item ? item.session_ids.map((id) => sessions.get(id)).filter((s) => s !== null) : [],
  );
  const dormant = $derived(itemSessions.some((s) => s.lifecycle === 'dormant'));
  let busy = $state<string | null>(null);

  $effect(() => {
    if (!work.loaded) untrack(() => void work.load().catch(() => {}));
  });

  async function resume(): Promise<void> {
    busy = 'resume';
    try {
      work.upsert(await workResume({ id: content.id }));
    } catch (err) {
      toasts.error(err, 'Resume');
    } finally {
      busy = null;
    }
  }

  async function retry(step: string): Promise<void> {
    busy = step;
    try {
      work.upsert(await workRetryStep({ id: content.id, step }));
    } catch (err) {
      toasts.error(err, 'Retry');
    } finally {
      busy = null;
    }
  }

  function browse(url: string): void {
    openExternal({ url }).catch((err) => toasts.error(err, 'Open in browser'));
  }
</script>

<div class="pane" data-testid="work-item-pane">
  {#if !item}
    <EmptyState
      icon="git-branch"
      title="Work item not found"
      body="It was finished and removed, or it belongs to another project."
    />
  {:else}
    <WorkItemHeader {projectId} {tabId} workItemId={content.id} />
    <div class="body">
      <dl class="facts">
        <dt>Branch</dt>
        <dd><code>{item.branch}</code> (base <code>{item.base}</code>)</dd>
        <dt>Worktree</dt>
        <dd><code>{item.worktree}</code></dd>
        <dt>Repository</dt>
        <dd>{item.repo_id}</dd>
        {#if item.pr_url}
          <dt>Pull request</dt>
          <dd>
            <button type="button" class="link" onclick={() => item.pr_url && browse(item.pr_url)}>
              {item.pr_url}
            </button>
          </dd>
        {/if}
        {#if item.state.kind === 'failed'}
          <dt>Failure</dt>
          <dd class="fail" role="alert">{stepLabel(item.state.step)}: {item.state.message}</dd>
        {/if}
      </dl>

      <h2>Steps</h2>
      <ol class="steps" aria-label="Steps">
        {#each item.steps as s (s.step)}
          <li data-status={s.status}>
            {#if s.status === 'done'}
              <Icon name="circle-check" size={14} />
            {:else if s.status === 'failed'}
              <Icon name="circle-x" size={14} />
            {:else if s.status === 'skipped'}
              <Icon name="minus" size={14} />
            {:else}
              <Icon name="circle" size={14} />
            {/if}
            <span>{stepLabel(s.step)}</span>
            {#if s.detail}<small>{s.detail}</small>{/if}
            {#if s.status === 'failed'}
              <Button size="sm" loading={busy === s.step} onclick={() => void retry(s.step)}>Retry</Button>
              <Button
                size="sm"
                variant="ghost"
                loading={busy === `skip:${s.step}`}
                onclick={() => void retry(`skip:${s.step}`)}>Skip</Button
              >
            {/if}
          </li>
        {/each}
      </ol>

      <h2>Sessions</h2>
      {#if itemSessions.length === 0}
        <p class="muted">No sessions are attached to this work item.</p>
      {:else}
        <ul class="sessions" aria-label="Sessions">
          {#each itemSessions as s (s.id)}
            <li>
              <span>{s.name}</span>
              <Badge
                tone={s.status === 'needs_input' ? 'danger' : s.status === 'working' ? 'info' : 'neutral'}
              >
                {s.status.replace('_', ' ')}
              </Badge>
              <small>{s.lifecycle}</small>
            </li>
          {/each}
        </ul>
      {/if}
      {#if dormant || (itemSessions.length === 0 && item.state.kind !== 'finished')}
        <Button icon="play" loading={busy === 'resume'} onclick={() => void resume()}>Resume sessions</Button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--k-bg);
    color: var(--k-fg);
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--k-space-3) var(--k-space-4);
  }

  h2 {
    margin: var(--k-space-4) 0 var(--k-space-2);
    font-size: var(--k-font-size);
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--k-space-1) var(--k-space-4);
    margin: 0;
  }

  .facts dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .fail {
    color: var(--k-danger);
  }

  .steps,
  .sessions {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-3);
    padding: 0;
    list-style: none;
  }

  .steps li,
  .sessions li {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .steps [data-status='done'] :global(.k-icon) {
    color: var(--k-ok);
  }

  .steps [data-status='failed'] {
    color: var(--k-danger);
  }

  .steps [data-status='pending'] {
    color: var(--k-fg-subtle);
  }

  small,
  .muted {
    color: var(--k-fg-subtle);
  }

  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-accent);
    font: inherit;
    cursor: pointer;
  }
</style>
