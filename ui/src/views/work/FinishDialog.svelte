<script lang="ts">
  import type { WorkItem } from '$lib/gen';
  import { workFinish } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, sessions, toasts, work } from '$lib/stores';
  import { Button, Dialog, Icon, Toggle } from '$lib/ui';

  import { parseDirtyFiles } from './common';

  interface Props {
    item: WorkItem;
    onclose: () => void;
    ondone?: (item: WorkItem) => void;
  }

  let { item, onclose, ondone }: Props = $props();

  const doneTarget = $derived(projects.byId(item.project_id)?.tracker?.status_map.done ?? null);
  let removeWorktree = $state(true);
  let deleteBranch = $state(false);
  let moveDone = $state(true);
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
          transition_to: moveDone ? doneTarget : null,
        },
      });
      work.upsert(updated);
      toasts.info(`Finished ${item.branch}`);
      ondone?.(updated);
      onclose();
    } catch (err) {
      const e = toIpcError('work_finish', err);
      if (e.code === 'dirty') {
        dirty = { files: parseDirtyFiles(e.detail), message: e.message };
      } else {
        toasts.error(err, 'Finish');
      }
    } finally {
      busy = false;
    }
  }
</script>

<Dialog
  title={dirty ? 'Worktree has changes' : `Finish ${item.branch}`}
  tone={dirty ? 'danger' : 'default'}
  width={480}
  {onclose}
>
  {#if dirty}
    <div class="body" data-testid="finish-dirty">
      <p class="msg"><Icon name="triangle-alert" size={14} /> {dirty.message}</p>
      {#if dirty.files.length > 0}
        <ul class="files" aria-label="Changed files">
          {#each dirty.files as f (f)}<li><code>{f}</code></li>{/each}
        </ul>
      {/if}
      <p class="hint">Removing the worktree now discards these changes.</p>
    </div>
  {:else}
    <div class="body">
      <p class="hint">
        Sessions of this work item are stopped, then the worktree is removed. It is refused while the worktree
        has uncommitted or unpushed changes.
      </p>
      {#if working.length > 0}
        <p class="msg" role="alert">
          <Icon name="triangle-alert" size={14} /> Claude is still working in {working.length === 1
            ? 'a session'
            : `${working.length} sessions`}. Finishing stops it.
        </p>
      {/if}
      <Toggle label="Remove the worktree" bind:checked={removeWorktree} />
      <Toggle label="Delete the local branch" bind:checked={deleteBranch} />
      {#if doneTarget && item.ticket}
        <Toggle label="Move the ticket to Done" bind:checked={moveDone} />
      {/if}
    </div>
  {/if}
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    {#if dirty}
      <Button variant="danger" loading={busy} onclick={() => void finish(true)}>Force remove</Button>
    {:else}
      <Button variant="primary" loading={busy} onclick={() => void finish(false)}>Finish</Button>
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
