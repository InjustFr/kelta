<script lang="ts">
  // Changes since you looked; for now its body holds the review notes (#133) from the former WorkItemPane.
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { NoteState, ReviewNote, ReviewNotes, WorkItem } from '$lib/gen';
  import { editorOpen, workNoteResolve, workNotes, workNotesSend } from '$lib/ipc/commands';
  import { toasts, work } from '$lib/stores';
  import { Badge, Button } from '$lib/ui';

  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  const NOTE_STATE: Record<NoteState, string> = {
    open: msg('open'),
    sent: msg('sent'),
    touched: msg('touched'),
    untouched: msg('untouched'),
    resolved: msg('resolved'),
  };

  const behind = $derived(work.git[item.id]?.behind ?? 0);
  const summary = $derived(
    [
      item.delta
        ? `+${item.delta.insertions} −${item.delta.deletions}, ${tn('{n} file', '{n} files', item.delta.files)}`
        : t('Nothing new'),
      item.branch,
      behind > 0 ? t('{n} behind', { n: behind }) : null,
    ]
      .filter(Boolean)
      .join(' · '),
  );

  let busy = $state(false);
  // Review notes: re-read on every `work.updated` of the item (nvim adds, Stop judges).
  let notes = $state<ReviewNotes | null>(null);
  $effect(() => {
    const id = item.id;
    untrack(() =>
      workNotes({ id }).then(
        (n) => {
          if (item.id === id) notes = n;
        },
        () => {},
      ),
    );
  });
  const shownNotes = $derived(notes?.notes.filter((n) => n.state !== 'resolved') ?? []);
  const openNotes = $derived(shownNotes.filter((n) => n.state === 'open' || n.state === 'untouched').length);

  async function sendNotes(): Promise<void> {
    busy = true;
    try {
      notes = await workNotesSend({ id: item.id });
    } catch (err) {
      toasts.error(err, t('Send review notes'));
    } finally {
      busy = false;
    }
  }

  async function resolveNote(n: ReviewNote): Promise<void> {
    try {
      notes = await workNoteResolve({ id: item.id, note: n.id });
    } catch (err) {
      toasts.error(err, t('Resolve note'));
    }
  }

  function jumpTo(n: ReviewNote): void {
    editorOpen({ target: { kind: 'work_item', id: item.id }, path: n.path, line: n.line_start }).catch(
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
</script>

{#snippet body()}
  <h3>
    {t('Review notes')}
    {#if notes?.since}
      <small>+{notes.since.insertions}/−{notes.since.deletions} {t('since feedback')}</small>
    {/if}
  </h3>
  <ul class="notes" aria-label={t('Review notes')} aria-keyshortcuts="s x e">
    {#each shownNotes as n (n.id)}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <li tabindex="0" data-state={n.state} onkeydown={(e) => noteKey(e, n)}>
        <input type="checkbox" aria-label={t('Resolve note')} onchange={() => void resolveNote(n)} />
        <button type="button" class="k-link" title={t('Open in editor (e)')} onclick={() => jumpTo(n)}>
          <code>{anchor(n)}</code>
        </button>
        <span class="text">{n.body}</span>
        {#if n.state !== 'open'}
          <Badge tone={n.state === 'untouched' ? 'warn' : n.state === 'touched' ? 'ok' : 'neutral'}
            >{t(NOTE_STATE[n.state] ?? n.state)}</Badge
          >
        {/if}
      </li>
    {/each}
  </ul>
  <p class="muted">
    {t("Touched / untouched is a heuristic: a change within 5 lines of the note after Claude's next stop.")}
    {t('Keys:')} <kbd>s</kbd>
    {t('send')}, <kbd>x</kbd>
    {t('resolve')}, <kbd>e</kbd>
    {t('open in editor')}.
  </p>
  <Button icon="send" disabled={openNotes === 0} loading={busy} onclick={() => void sendNotes()}
    >{tn('Send {n} open note to Claude', 'Send {n} open notes to Claude', openNotes)}</Button
  >
{/snippet}

<Section
  title={t('Changes')}
  {summary}
  {open}
  testid="story-changes"
  children={shownNotes.length > 0 ? body : undefined}
/>

<style>
  h3 {
    margin: 0 0 var(--k-space-2);
    font-size: var(--k-font-size-sm);
  }

  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-3);
    padding: 0;
    list-style: none;
  }

  .notes li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
  }

  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  [data-state='untouched'] .text {
    color: var(--k-warn);
  }

  small,
  .muted {
    color: var(--k-fg-subtle);
  }
</style>
