<script lang="ts">
  // Fix with Claude (FLOW §4.2): the PR's review feedback, what is new since the last send checked,
  // an editable prompt, and one `⌘↵` that writes feedback.md to the private run dir and resumes the
  // item's previous Claude (held until its Stop while it works).
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { Feedback, KeltaError, WorkItemId } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { isIpcError, toIpcError } from '$lib/ipc/transport';
  import { sessions, toasts, work } from '$lib/stores';
  import { Button, Sheet, Spinner, TextInput } from '$lib/ui';
  import { currentPlatform, relativeTime } from '$lib/ui/format';

  import {
    feedbackEntries,
    feedbackMarkdown,
    isNew,
    newestAt,
    prLabel,
    promptTemplate,
    rememberReviewers,
    sendDeferred,
    type FeedbackEntry,
  } from './fixloop.svelte';
  import { openContent } from './nav';

  interface Props extends SheetProps {
    id: WorkItemId;
  }

  let { onclose, id }: Props = $props();

  const submitChord = currentPlatform() === 'macos' ? 'cmd+enter' : 'ctrl+enter';

  const item = $derived(work.get(id));
  const claude = $derived(
    sessions.all.find((s) => s.work_item_id === id && s.kind.type === 'claude' && s.lifecycle === 'live') ??
      null,
  );
  const hooksInactive = $derived(claude !== null && claude.status_source !== 'hook');
  // Held by core until Claude's next Stop.
  const busy = $derived(
    !hooksInactive && claude !== null && ['working', 'needs_input'].includes(claude.status),
  );

  let feedback = $state<Feedback | null>(null);
  let loadError = $state<KeltaError | null>(null);
  let withoutFeedback = $state(false);
  let checked = $state<Record<string, boolean>>({});
  /** New since the last send, fixed at load: the rest lists under "Seen before". */
  let fresh = $state<Record<string, boolean>>({});
  let prompt = $state(untrack(() => promptTemplate('feedback')));
  let sending = $state(false);
  let sendError = $state<string | null>(null);
  let list = $state<HTMLElement>();

  const entries = $derived<FeedbackEntry[]>(feedback ? feedbackEntries(feedback) : []);
  const chosen = $derived(entries.filter((e) => checked[e.key]));
  const groups = $derived(
    (
      [
        ['thread', 'Unresolved threads'],
        ['review', 'Reviews'],
        ['check', 'Failed checks'],
      ] as const
    )
      .map(([kind, label]) => {
        const rows = entries.filter((e) => e.kind === kind);
        return {
          kind,
          label,
          rows,
          news: rows.filter((e) => fresh[e.key]),
          seen: rows.filter((e) => !fresh[e.key]),
        };
      })
      .filter((g) => g.rows.length > 0),
  );
  const ready = $derived((feedback !== null || withoutFeedback) && !sending);

  $effect(() => {
    untrack(() => void load());
  });

  async function load(): Promise<void> {
    loadError = null;
    try {
      const fb = await ipc.workFeedback({ id });
      feedback = fb;
      const all = feedbackEntries(fb);
      const since = untrack(() => item?.feedback_at);
      fresh = Object.fromEntries(all.map((e) => [e.key, isNew(e, since)]));
      checked = { ...fresh };
      if (all.length > 0 && all.every((e) => e.kind === 'check') && prompt === promptTemplate('feedback'))
        prompt = promptTemplate('checks');
      rememberReviewers(id, fb);
    } catch (err) {
      loadError = toIpcError('work_feedback', err).toKeltaError();
    }
  }

  async function send(): Promise<void> {
    if (!ready || !item) return;
    sending = true;
    sendError = null;
    try {
      const files = feedback ? [{ name: 'feedback.md', content: feedbackMarkdown(chosen) }] : [];
      const threads = chosen.flatMap((e) => (e.threadId ? [e.threadId] : []));
      const deferred = sendDeferred(id);
      // Every entry shown, ticked or not: a deliberately unticked thread does not nag again.
      const feedback_at = newestAt(entries);
      work.upsert(await ipc.workSend({ id, prompt, files, threads, feedback_at }));
      void work.refreshStatus();
      if (!deferred)
        toasts.info(`Sent ${chosen.length} feedback item${chosen.length === 1 ? '' : 's'} to Claude.`);
      onclose();
    } catch (err) {
      sendError = isIpcError(err) ? err.message : toIpcError('work_send', err).message;
    } finally {
      sending = false;
    }
  }

  async function copyPrompt(): Promise<void> {
    const text = feedback ? `${prompt}\n\n${feedbackMarkdown(chosen)}` : prompt;
    try {
      await ipc.clipboardWrite({ kind: 'clipboard', text });
      toasts.info('Prompt copied.');
    } catch (err) {
      toasts.error(err, 'Copy prompt');
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void send();
      return;
    }
    if ((e.key !== 'j' && e.key !== 'k') || e.metaKey || e.ctrlKey || e.altKey) return;
    if ((e.target as HTMLElement).closest('textarea, input[type="text"]')) return;
    const boxes = [...(list?.querySelectorAll<HTMLInputElement>('input[type="checkbox"]') ?? [])];
    if (boxes.length === 0) return;
    const at = boxes.indexOf(document.activeElement as HTMLInputElement);
    const next = e.key === 'j' ? Math.min(at + 1, boxes.length - 1) : Math.max(at - 1, 0);
    boxes[at < 0 ? 0 : next]?.focus();
    e.preventDefault();
  }

  function age(iso: string | undefined): string {
    const t = iso ? Date.parse(iso) : NaN;
    return Number.isNaN(t) ? '' : ` from ${relativeTime(t)}`;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="host" {onkeydown}>
  <Sheet title="Fix with Claude" width={480} {onclose}>
    {#if item}
      <p class="subject">
        <code>{item.branch}</code>
        <span>PR {prLabel(item)}</span>
      </p>

      <div class="prompt">
        <TextInput label="Prompt" multiline rows={4} bind:value={prompt} data-testid="fix-prompt" />
      </div>

      <section class="feedback" aria-label="Review feedback" bind:this={list}>
        {#if loadError}
          <div class="problem" role="alert">
            <p>{loadError.message}</p>
            <div class="row-actions">
              <Button size="sm" onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
                Open account settings
              </Button>
              <Button size="sm" onclick={() => (withoutFeedback = true)} disabled={withoutFeedback}>
                Send without feedback
              </Button>
            </div>
          </div>
        {:else if !feedback}
          <p class="muted"><Spinner size={12} /> Reading the review feedback</p>
        {:else if entries.length === 0}
          <p class="muted">
            No unresolved threads, review summaries or failed checks. The prompt goes alone.
          </p>
        {:else}
          {#each groups as g (g.kind)}
            <h3>{g.label} <span class="k-num">{g.rows.length}</span></h3>
            {#snippet rows(list: FeedbackEntry[])}
              <ul>
                {#each list as e (e.key)}
                  <li class:off={!checked[e.key]}>
                    <label>
                      <input type="checkbox" bind:checked={checked[e.key]} data-testid="fix-item" />
                      <span class="title">{e.title}</span>
                    </label>
                    {#if e.kind === 'check'}
                      <pre class="body log">{e.text}</pre>
                    {:else}
                      <p class="body">{e.text}</p>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/snippet}
            {@render rows(g.news)}
            {#if g.seen.length > 0}
              <h4>Seen before</h4>
              {@render rows(g.seen)}
            {/if}
          {/each}
        {/if}
      </section>

      {#if hooksInactive}
        <div class="problem" role="status">
          <p>Kelta can't tell whether Claude is idle (status hooks inactive): sending copies the prompt.</p>
          <div class="row-actions">
            <Button
              size="sm"
              onclick={() =>
                void openContent(item.project_id, { kind: 'diagnostics' }, { placement: 'new_tab' })}
            >
              Fix hooks
            </Button>
            <Button size="sm" onclick={() => void copyPrompt()}>Copy prompt</Button>
          </div>
        </div>
      {/if}
      {#if sendError}<p class="problem" role="alert">{sendError}</p>{/if}
    {:else}
      <p class="muted">This work item is gone.</p>
    {/if}

    {#snippet actions()}
      <span class="resumes" data-testid="fix-resumes">
        {#if busy}
          Claude is working: it gets this when it stops.
        {:else if item?.claude_uuid}
          Resumes conversation <code>{item.claude_uuid.slice(0, 8)}</code>{age(item.created_at)}
        {/if}
      </span>
      <Button variant="ghost" onclick={onclose}>Cancel</Button>
      <Button
        variant="primary"
        disabled={!ready}
        loading={sending}
        chord={submitChord}
        onclick={() => void send()}>{hooksInactive ? 'Copy for Claude' : 'Send to Claude'}</Button
      >
    {/snippet}
  </Sheet>
</div>

<style>
  .subject {
    display: flex;
    gap: var(--k-space-3);
    margin: 0 0 var(--k-space-3);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .prompt {
    margin-bottom: var(--k-space-4);
  }

  h3 {
    margin: var(--k-space-3) 0 var(--k-space-1);
    font-size: var(--k-font-size-sm);
    font-weight: 600;
    color: var(--k-fg-muted);
  }

  h4 {
    margin: var(--k-space-2) 0 var(--k-space-1);
    font-size: var(--k-font-size-xs);
    font-weight: 600;
    color: var(--k-fg-subtle);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    padding: var(--k-space-1) 0 var(--k-space-2);
  }

  li.off .title,
  li.off .body {
    color: var(--k-fg-subtle);
  }

  label {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-2);
    cursor: pointer;
  }

  .title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .body {
    margin: var(--k-space-1) 0 0 calc(13px + var(--k-space-2));
    max-height: 7.5em;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--k-fg-muted);
  }

  .log {
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xs);
  }

  .muted {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    color: var(--k-fg-muted);
  }

  .problem {
    margin: var(--k-space-3) 0 0;
    color: var(--k-fg);
  }

  .problem p {
    margin: 0 0 var(--k-space-2);
  }

  .row-actions {
    display: flex;
    gap: var(--k-space-2);
  }

  .resumes {
    flex: 1;
    align-self: center;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }
</style>
