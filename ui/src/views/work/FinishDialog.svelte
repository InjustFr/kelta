<script lang="ts">
  import { t, tn } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { WorkItem } from '$lib/gen';
  import { workFinish } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, sessions, tickets, toasts, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Button, Dialog, Icon, Select, Toggle } from '$lib/ui';

  import { parseDirtyFiles } from './common';

  interface Props {
    item: WorkItem;
    onclose: () => void;
    ondone?: (item: WorkItem) => void;
  }

  let { item, onclose, ondone }: Props = $props();

  const doneTarget = $derived(projects.byId(item.project_id)?.tracker?.status_map.done ?? null);
  // Merged / closed on the host: prefilled cleanup (FLOW §4.6). The ticket was already moved on
  // merge when unambiguous; otherwise the Done status is chosen here, never guessed.
  const ended = $derived(item.state.kind === 'merged' || item.state.kind === 'pr_closed');
  const endDetail = $derived(item.state.kind === 'merged' ? item.state.detail : null);
  const choose = $derived(!!item.ticket && (item.state.kind === 'pr_closed' || endDetail !== null));
  const doneChoices = $derived(
    item.ticket
      ? (tickets.transitions[ticketKey(item.ticket)]?.data ?? []).filter((t) => t.to.category === 'done')
      : [],
  );
  let removeWorktree = $state(true);
  let deleteBranch = $state(untrack(() => ended));
  let moveDone = $state(true);
  /** Chosen Done status name; '' leaves the ticket as is. */
  let doneName = $state('');

  $effect(() => {
    const ref = item.ticket;
    if (choose && ref) untrack(() => void tickets.loadTransitions(ref));
  });

  const transitionTo = $derived(
    ended ? (doneName ? { name: doneName } : null) : moveDone ? doneTarget : null,
  );
  let busy = $state(false);
  let dirty = $state<{ files: string[]; message: string } | null>(null);

  const working = $derived(
    item.session_ids.map((id) => sessions.get(id)).filter((s) => s?.status === 'working'),
  );

  async function finish(force: boolean): Promise<void> {
    busy = true;
    try {
      const updated = await workFinish({
        id: item.id,
        opts: {
          remove_worktree: removeWorktree,
          delete_branch: deleteBranch,
          force,
          transition_to: transitionTo,
        },
      });
      work.upsert(updated);
      toasts.info(t('Finished {branch}', { branch: item.branch }));
      ondone?.(updated);
      onclose();
    } catch (err) {
      const e = toIpcError('work_finish', err);
      if (e.code === 'dirty') {
        dirty = { files: parseDirtyFiles(e.detail), message: e.message };
      } else {
        toasts.error(err, t('Finish'));
      }
    } finally {
      busy = false;
    }
  }
</script>

<Dialog
  title={dirty ? t('Worktree has changes') : t('Finish {branch}', { branch: item.branch })}
  tone={dirty ? 'danger' : 'default'}
  width={480}
  {onclose}
>
  {#if dirty}
    <div class="body" data-testid="finish-dirty">
      <p class="msg"><Icon name="triangle-alert" size={14} /> {dirty.message}</p>
      {#if dirty.files.length > 0}
        <ul class="files" aria-label={t('Changed files')}>
          {#each dirty.files as f (f)}<li><code>{f}</code></li>{/each}
        </ul>
      {/if}
      <p class="hint">{t('Removing the worktree now discards these changes.')}</p>
    </div>
  {:else}
    <div class="body">
      <p class="hint">
        {t(
          'Sessions of this work item are stopped, then the worktree is removed. It is refused while the worktree has uncommitted or unpushed changes.',
        )}
      </p>
      {#if working.length > 0}
        <p class="msg" role="alert">
          <Icon name="triangle-alert" size={14} />
          {tn(
            'Claude is still working in a session. Finishing stops it.',
            'Claude is still working in {n} sessions. Finishing stops it.',
            working.length,
          )}
        </p>
      {/if}
      {#if endDetail}
        <p class="msg" data-testid="finish-detail"><Icon name="info" size={14} /> {endDetail}</p>
      {/if}
      <Toggle label={t('Remove the worktree')} bind:checked={removeWorktree} />
      <Toggle label={t('Delete the local branch')} bind:checked={deleteBranch} />
      {#if choose}
        <Select
          label={t('Done status')}
          bind:value={doneName}
          options={[
            { value: '', label: t('Leave the ticket as is') },
            ...doneChoices.map((t) => ({ value: t.to.name, label: t.to.name })),
          ]}
        />
      {:else if doneTarget && item.ticket && !ended}
        <Toggle label={t('Move the ticket to Done')} bind:checked={moveDone} />
      {/if}
    </div>
  {/if}
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>{t('Cancel')}</Button>
    {#if dirty}
      <Button variant="danger" loading={busy} onclick={() => void finish(true)}>{t('Force remove')}</Button>
    {:else}
      <Button variant="primary" loading={busy} onclick={() => void finish(false)}>{t('Finish')}</Button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .body {
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

  .files {
    max-height: 200px;
    margin: 0;
    padding: var(--k-space-2) var(--k-space-4);
    overflow: auto;
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
  }
</style>
