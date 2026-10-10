<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import type { ReviewNote, ReviewNotes } from '$lib/gen';
  import {
    editorOpen,
    openExternal,
    workNoteResolve,
    workNotes,
    workNotesSend,
    workResume,
    workRetryStep,
  } from '$lib/ipc/commands';
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

  // Review notes (#133): re-read on every `work.updated` of the item (nvim adds, Stop judges).
  let notes = $state<ReviewNotes | null>(null);
  $effect(() => {
    if (!item) return;
    const id = item.id;
    untrack(() =>
      workNotes({ id }).then(
        (n) => {
          if (content.id === id) notes = n;
        },
        () => {},
      ),
    );
  });
  const shownNotes = $derived(notes?.notes.filter((n) => n.state !== 'resolved') ?? []);
  const openNotes = $derived(shownNotes.filter((n) => n.state === 'open' || n.state === 'untouched').length);

  async function sendNotes(): Promise<void> {
    busy = 'notes';
    try {
      notes = await workNotesSend({ id: content.id });
    } catch (err) {
      toasts.error(err, 'Send review notes');
    } finally {
      busy = null;
    }
  }

  async function resolveNote(n: ReviewNote): Promise<void> {
    try {
      notes = await workNoteResolve({ id: content.id, note: n.id });
    } catch (err) {
      toasts.error(err, 'Resolve note');
    }
  }

  function jumpTo(n: ReviewNote): void {
    editorOpen({ target: { kind: 'work_item', id: content.id }, path: n.path, line: n.line_start }).catch(
      (err) => toasts.error(err, 'Open in editor'),
    );
  }

  // `s` sends the open notes, `x` resolves and `e` opens the focused one in nvim.
  function noteKey(e: KeyboardEvent, n: ReviewNote): void {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key === 's') void sendNotes();
    else if (e.key === 'x') void resolveNote(n);
    else if (e.key === 'e') jumpTo(n);
    else return;
    e.preventDefault();
  }

  const anchor = (n: ReviewNote): string =>
    `${n.path}#L${n.line_start}${n.line_end > n.line_start ? `-${n.line_end}` : ''}`;

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

      {#if shownNotes.length > 0}
        <h2>
          Review notes
          {#if notes?.since}
            <small>+{notes.since.insertions}/−{notes.since.deletions} since feedback</small>
          {/if}
        </h2>
        <ul class="notes" aria-label="Review notes" aria-keyshortcuts="s x e">
          {#each shownNotes as n (n.id)}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <li tabindex="0" data-state={n.state} onkeydown={(e) => noteKey(e, n)}>
              <input type="checkbox" aria-label="Resolve note" onchange={() => void resolveNote(n)} />
              <button type="button" class="link" title="Open in editor (e)" onclick={() => jumpTo(n)}>
                <code>{anchor(n)}</code>
              </button>
              <span class="body">{n.body}</span>
              {#if n.state !== 'open'}
                <Badge tone={n.state === 'untouched' ? 'warn' : n.state === 'touched' ? 'ok' : 'neutral'}
                  >{n.state}</Badge
                >
              {/if}
            </li>
          {/each}
        </ul>
        <p class="muted">
          Touched / untouched is a heuristic: a change within 5 lines of the note after Claude's next stop.
          Keys: <kbd>s</kbd> send, <kbd>x</kbd> resolve, <kbd>e</kbd> open in editor.
        </p>
        <Button
          icon="send"
          disabled={openNotes === 0}
          loading={busy === 'notes'}
          onclick={() => void sendNotes()}
          >Send {openNotes} open {openNotes === 1 ? 'note' : 'notes'} to Claude</Button
        >
      {/if}

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
  .notes,
  .sessions {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-3);
    padding: 0;
    list-style: none;
  }

  .steps li,
  .notes li,
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

  .notes .body {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .notes [data-state='untouched'] .body {
    color: var(--k-warn);
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
