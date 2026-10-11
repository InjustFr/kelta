<script lang="ts">
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import type { Lifecycle, NoteState, ReviewNote, ReviewNotes } from '$lib/gen';
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

  import { statusLabel } from '../../shell/labels';
  import { stepLabel } from './common';

  let { content }: PaneProps<'work_item'> = $props();

  const LIFECYCLE: Record<Lifecycle, string> = {
    live: msg('live'),
    dormant: msg('dormant'),
    exited: msg('exited'),
  };
  const NOTE_STATE: Record<NoteState, string> = {
    open: msg('open'),
    sent: msg('sent'),
    touched: msg('touched'),
    untouched: msg('untouched'),
    resolved: msg('resolved'),
  };

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
      toasts.error(err, t('Send review notes'));
    } finally {
      busy = null;
    }
  }

  async function resolveNote(n: ReviewNote): Promise<void> {
    try {
      notes = await workNoteResolve({ id: content.id, note: n.id });
    } catch (err) {
      toasts.error(err, t('Resolve note'));
    }
  }

  function jumpTo(n: ReviewNote): void {
    editorOpen({ target: { kind: 'work_item', id: content.id }, path: n.path, line: n.line_start }).catch(
      (err) => toasts.error(err, t('Open in editor')),
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
      toasts.error(err, t('Resume'));
    } finally {
      busy = null;
    }
  }

  async function retry(step: string): Promise<void> {
    busy = step;
    try {
      work.upsert(await workRetryStep({ id: content.id, step }));
    } catch (err) {
      toasts.error(err, t('Retry'));
    } finally {
      busy = null;
    }
  }

  function browse(url: string): void {
    openExternal({ url }).catch((err) => toasts.error(err, t('Open in browser')));
  }
</script>

<div class="pane" data-testid="work-item-pane">
  {#if !item}
    <EmptyState
      icon="git-branch"
      title={t('Work item not found')}
      body={t('It was finished and removed, or it belongs to another project.')}
    />
  {:else}
    <div class="body">
      <dl class="facts">
        <dt>{t('Branch')}</dt>
        <dd><code>{item.branch}</code> ({t('base')} <code>{item.base}</code>)</dd>
        <dt>{t('Worktree')}</dt>
        <dd><code>{item.worktree}</code></dd>
        <dt>{t('Repository')}</dt>
        <dd>{item.repo_id}</dd>
        {#if item.pr_url}
          <dt>{t('Pull request')}</dt>
          <dd>
            <button type="button" class="k-link" onclick={() => item.pr_url && browse(item.pr_url)}>
              {item.pr_url}
            </button>
          </dd>
        {/if}
        {#if item.state.kind === 'failed'}
          <dt>{t('Failure')}</dt>
          <dd class="fail" role="alert">{stepLabel(item.state.step)}: {item.state.message}</dd>
        {/if}
      </dl>

      <h2>{t('Steps')}</h2>
      <ol class="steps" aria-label={t('Steps')}>
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
              <Button size="sm" loading={busy === s.step} onclick={() => void retry(s.step)}
                >{t('Retry')}</Button
              >
              <Button
                size="sm"
                variant="ghost"
                loading={busy === `skip:${s.step}`}
                onclick={() => void retry(`skip:${s.step}`)}>{t('Skip')}</Button
              >
            {/if}
          </li>
        {/each}
      </ol>

      {#if shownNotes.length > 0}
        <h2>
          {t('Review notes')}
          {#if notes?.since}
            <small>+{notes.since.insertions}/−{notes.since.deletions} {t('since feedback')}</small>
          {/if}
        </h2>
        <ul class="notes" aria-label={t('Review notes')} aria-keyshortcuts="s x e">
          {#each shownNotes as n (n.id)}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <li tabindex="0" data-state={n.state} onkeydown={(e) => noteKey(e, n)}>
              <input type="checkbox" aria-label={t('Resolve note')} onchange={() => void resolveNote(n)} />
              <button type="button" class="k-link" title={t('Open in editor (e)')} onclick={() => jumpTo(n)}>
                <code>{anchor(n)}</code>
              </button>
              <span class="body">{n.body}</span>
              {#if n.state !== 'open'}
                <Badge tone={n.state === 'untouched' ? 'warn' : n.state === 'touched' ? 'ok' : 'neutral'}
                  >{t(NOTE_STATE[n.state] ?? n.state)}</Badge
                >
              {/if}
            </li>
          {/each}
        </ul>
        <p class="muted">
          {t(
            "Touched / untouched is a heuristic: a change within 5 lines of the note after Claude's next stop.",
          )}
          {t('Keys:')} <kbd>s</kbd>
          {t('send')}, <kbd>x</kbd>
          {t('resolve')}, <kbd>e</kbd>
          {t('open in editor')}.
        </p>
        <Button
          icon="send"
          disabled={openNotes === 0}
          loading={busy === 'notes'}
          onclick={() => void sendNotes()}
          >{tn('Send {n} open note to Claude', 'Send {n} open notes to Claude', openNotes)}</Button
        >
      {/if}

      <h2>{t('Sessions')}</h2>
      {#if itemSessions.length === 0}
        <p class="muted">{t('No sessions are attached to this work item.')}</p>
      {:else}
        <ul class="sessions" aria-label={t('Sessions')}>
          {#each itemSessions as s (s.id)}
            <li>
              <span>{s.name}</span>
              <Badge
                tone={s.status === 'needs_input' ? 'danger' : s.status === 'working' ? 'info' : 'neutral'}
              >
                {statusLabel(s.status)}
              </Badge>
              <small>{t(LIFECYCLE[s.lifecycle])}</small>
            </li>
          {/each}
        </ul>
      {/if}
      {#if dormant || (itemSessions.length === 0 && item.state.kind !== 'finished')}
        <Button icon="play" loading={busy === 'resume'} onclick={() => void resume()}
          >{t('Resume sessions')}</Button
        >
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
</style>
