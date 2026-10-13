<script lang="ts">
  // Changes since you looked (#425): the files of `reviewed..last` (a click or `]f` / `[f` opens the
  // delta at that file), Review / Mark reviewed / a note to Claude, then the review notes (#133).
  import { msg, t, tn } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { DeltaFile, NoteState, ReviewNote, ReviewNotes, WorkItem } from '$lib/gen';
  import { editorOpen, workDeltaFiles, workNoteResolve, workNotes, workNotesSend } from '$lib/ipc/commands';
  import { toasts, work } from '$lib/stores';
  import { Badge, Button, TextInput } from '$lib/ui';

  import { reviewDeltaFile, runWorkAction } from '../actions';
  import { sendToClaude } from '../fixloop.svelte';
  import { claudeOf } from '../live';
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
  const working = $derived(claudeOf(item)?.status === 'working');

  let busy = $state(false);
  // Delta files and review notes: re-read on every `work.updated` of the item (Stop, Mark reviewed, nvim).
  let files = $state<DeltaFile[]>([]);
  let notes = $state<ReviewNotes | null>(null);
  $effect(() => {
    const id = item.id;
    untrack(() => {
      workDeltaFiles({ id }).then(
        (f) => {
          if (item.id === id) files = f;
        },
        () => {},
      );
      workNotes({ id }).then(
        (n) => {
          if (item.id === id) notes = n;
        },
        () => {},
      );
    });
  });
  const real = $derived(files.filter((f) => !f.generated));
  // Lockfiles and generated files fold into one row, as in the delta chip.
  const generated = $derived.by(() => {
    const g = files.filter((f) => f.generated);
    return g.length === 0
      ? null
      : {
          n: g.length,
          insertions: g.reduce((s, f) => s + f.insertions, 0),
          deletions: g.reduce((s, f) => s + f.deletions, 0),
        };
  });
  const shownNotes = $derived(notes?.notes.filter((n) => n.state !== 'resolved') ?? []);
  const openNotes = $derived(shownNotes.filter((n) => n.state === 'open' || n.state === 'untouched').length);

  function openFile(path: string): void {
    reviewDeltaFile(item, path).catch((err) => toasts.error(err, t('Review changes')));
  }

  // `]f` / `[f`: the next / previous file, focused and opened.
  let fileList = $state<HTMLElement>();
  let bracket: string | null = null;
  function stepFile(dir: 1 | -1): void {
    const rows = [...(fileList?.querySelectorAll<HTMLButtonElement>('button[data-path]') ?? [])];
    if (rows.length === 0) return;
    const at = rows.indexOf(document.activeElement as HTMLButtonElement);
    const row =
      rows[at < 0 ? (dir > 0 ? 0 : rows.length - 1) : Math.min(rows.length - 1, Math.max(0, at + dir))]!;
    row.focus();
    openFile(row.dataset.path!);
  }

  // `v` reviews the delta, `R` marks it reviewed, `]f` / `[f` walk the files; never from the note field.
  function sectionKey(e: KeyboardEvent): void {
    if (e.metaKey || e.ctrlKey || e.altKey || (e.target as HTMLElement).closest('input, textarea')) return;
    const prev = bracket;
    bracket = e.key === ']' || e.key === '[' ? e.key : null;
    if (bracket) return e.preventDefault();
    if (e.key === 'f' && prev) stepFile(prev === ']' ? 1 : -1);
    else if (e.key === 'v') void runWorkAction('review_delta', item);
    else if (e.key === 'R' && !working) void runWorkAction('mark_reviewed', item);
    else return;
    e.preventDefault();
  }

  let note = $state('');
  async function sendNote(): Promise<void> {
    const text = note.trim();
    if (text && (await sendToClaude(item, text))) note = '';
  }

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
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div onkeydown={sectionKey}>
    {#if files.length > 0}
      <ul class="files" bind:this={fileList} aria-label={t('Changed files')} aria-keyshortcuts="] [ v R">
        {#each real as f (f.path)}
          <li>
            <button
              type="button"
              class="k-link"
              data-path={f.path}
              title={t('Review changes at this file')}
              onclick={() => openFile(f.path)}><code>{f.path}</code></button
            >
            {#if f.test}<Badge tone="ok">{t('test')}</Badge>{/if}
            <span class="num">+{f.insertions} −{f.deletions}</span>
          </li>
        {/each}
        {#if generated}
          <li class="muted">
            <span class="path"
              >{tn('{n} lockfile or generated file', '{n} lockfiles or generated files', generated.n)}</span
            >
            <span class="num">+{generated.insertions} −{generated.deletions}</span>
          </li>
        {/if}
      </ul>
      <div class="actions">
        <Button size="sm" chord="v" onclick={() => void runWorkAction('review_delta', item)}
          >{t('Review changes')}</Button
        >
        {#if !working}
          <Button
            size="sm"
            chord="R"
            disabled={!item.review_due}
            onclick={() => void runWorkAction('mark_reviewed', item)}>{t('Mark reviewed')}</Button
          >
        {/if}
      </div>
      <TextInput
        bind:value={note}
        placeholder={t('Note to Claude')}
        aria-label={t('Note to Claude')}
        onkeydown={(e: KeyboardEvent) => {
          if (e.key === 'Enter' && !e.isComposing) {
            e.preventDefault();
            void sendNote();
          }
        }}
      />
    {/if}
    {#if shownNotes.length > 0}
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
        {t(
          "Touched / untouched is a heuristic: a change within 5 lines of the note after Claude's next stop.",
        )}
        {t('Keys:')} <kbd>s</kbd>
        {t('send')}, <kbd>x</kbd>
        {t('resolve')}, <kbd>e</kbd>
        {t('open in editor')}.
      </p>
      <Button icon="send" disabled={openNotes === 0} loading={busy} onclick={() => void sendNotes()}
        >{tn('Send {n} open note to Claude', 'Send {n} open notes to Claude', openNotes)}</Button
      >
    {/if}
  </div>
{/snippet}

<Section
  title={working ? t('Changes so far') : t('Changes')}
  {summary}
  {open}
  testid="story-changes"
  children={files.length > 0 || shownNotes.length > 0 ? body : undefined}
/>

<style>
  h3 {
    margin: var(--k-space-3) 0 var(--k-space-2);
    font-size: var(--k-font-size-sm);
  }

  .files,
  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-3);
    padding: 0;
    list-style: none;
  }

  .files li {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .files button,
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .num {
    flex: none;
    color: var(--k-fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2);
    margin-bottom: var(--k-space-2);
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
