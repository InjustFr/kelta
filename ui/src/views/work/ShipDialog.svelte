<script lang="ts">
  import { untrack } from 'svelte';

  import type { GitStatus, WorkItem } from '$lib/gen';
  import { sessionWrite, workCreatePr, workPrDraft, workStatus } from '$lib/ipc/commands';
  import { sessions, toasts, work } from '$lib/stores';
  import { Button, Dialog, Icon, TextInput, Toggle } from '$lib/ui';

  import { claudeBusy, prNumber } from './shipPhase';

  interface Props {
    item: WorkItem;
    onclose: () => void;
  }

  let { item, onclose }: Props = $props();

  let title = $state('');
  let body = $state('');
  let draft = $state(false);
  let git = $state<GitStatus | null>(null);
  let loading = $state(true);
  let busy = $state(false);

  const claude = $derived(
    item.session_ids
      .map((id) => sessions.get(id))
      .find((s) => s?.kind.type === 'claude' && s.lifecycle !== 'exited') ?? null,
  );
  const working = $derived(claudeBusy(item, (id) => sessions.get(id)));
  const nothing = $derived(git !== null && git.ahead === 0);
  // Refusals first: they disable Ship (FLOW §4.5).
  const blocked = $derived(
    working
      ? 'Claude is working in this worktree. Ship when it stops.'
      : nothing
        ? `No commits ahead of ${item.base}.`
        : null,
  );

  $effect(() => {
    const id = item.id;
    untrack(() => void load(id));
  });

  async function load(id: string): Promise<void> {
    const [d, g] = await Promise.allSettled([workPrDraft({ id }), workStatus({ id })]);
    if (d.status === 'fulfilled') {
      // Keep what was typed while the defaults loaded.
      title ||= d.value.title ?? '';
      body ||= d.value.body ?? '';
      draft = d.value.draft ?? false;
    }
    // Unknown git state: let the backend decide (it refuses an empty Ship too).
    git = g.status === 'fulfilled' ? g.value : null;
    loading = false;
  }

  async function ship(): Promise<void> {
    if (busy || blocked) return;
    busy = true;
    try {
      const updated = work.upsert(
        await workCreatePr({ id: item.id, draft: { title: title.trim() || null, body, draft } }),
      );
      const url = updated.pr_url;
      toasts.push({
        level: 'info',
        text: `Opened PR ${prNumber(url)}`.trim(),
        action: url ? { label: 'Open', command: 'tickets.open_in_browser', args: { url } } : null,
      });
      onclose();
    } catch (err) {
      toasts.error(err, 'Ship');
    } finally {
      busy = false;
    }
  }

  async function askToCommit(): Promise<void> {
    if (!claude) return;
    try {
      await sessionWrite(claude.id, 'Commit the current changes with a clear message. Do not push.\r');
      toasts.info('Asked Claude to commit. Ship when it stops.');
      onclose();
    } catch (err) {
      toasts.error(err, 'Ask Claude to commit');
    }
  }
</script>

<Dialog title="Ship {item.ticket?.key ?? item.branch}" width={520} {onclose}>
  <div class="form" data-testid="ship-dialog">
    <p class="hint">
      Pushes <code>{item.branch}</code> and opens a pull request against <code>{item.base}</code>.
    </p>
    {#if blocked}
      <p class="msg" role="alert"><Icon name="triangle-alert" size={14} /> {blocked}</p>
    {:else if git?.dirty}
      <p class="msg" data-testid="ship-dirty">
        <Icon name="triangle-alert" size={14} /> Uncommitted changes will not be in the PR.
      </p>
    {/if}
    <TextInput label="Title" bind:value={title} />
    <TextInput label="Description" bind:value={body} multiline rows={5} />
    <Toggle label="Draft" bind:checked={draft} />
  </div>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    {#if git?.dirty && !blocked}
      <Button disabled={!claude} onclick={() => void askToCommit()}>Ask Claude to commit</Button>
    {/if}
    <Button variant="primary" loading={busy} disabled={loading || !!blocked} onclick={() => void ship()}>
      {git?.dirty ? 'Ship anyway' : 'Ship'}
    </Button>
  {/snippet}
</Dialog>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .hint {
    margin: 0;
    color: var(--k-fg-muted);
  }

  .msg {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: 0;
    color: var(--k-warn);
  }
</style>
