<script lang="ts">
  import { untrack } from 'svelte';

  import type { MergeMethod, WorkItem } from '$lib/gen';
  import { openExternal, workArmMerge } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { settings, toasts, work } from '$lib/stores';
  import { Button, Dialog, Icon, Select } from '$lib/ui';

  import { prOf } from './live';

  interface Props {
    item: WorkItem;
    onclose: () => void;
  }

  let { item, onclose }: Props = $props();

  let method = $state<MergeMethod>(
    untrack(() => settings.value(item.project_id)?.work.pr.merge_method ?? 'squash'),
  );
  let busy = $state(false);
  /** The host's refusal (auto-merge off for the repo, missing permission…). */
  let refused = $state<string | null>(null);
  const url = $derived(item.pr_url ?? prOf(item)?.url ?? null);

  async function arm(): Promise<void> {
    busy = true;
    refused = null;
    try {
      work.upsert(await workArmMerge({ id: item.id, method }));
      toasts.info(`${item.ticket?.key ?? item.branch}: merges when ready, then finishes`);
      onclose();
    } catch (err) {
      refused = toIpcError('work_arm_merge', err).message;
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title="Merge {item.ticket?.key ?? item.branch} when ready" width={480} {onclose}>
  <div class="form" data-testid="merge-dialog">
    <p class="hint">
      The code host merges the pull request once approvals and checks pass. Kelta then stops the sessions,
      removes the worktree if it is clean and pushed, deletes the branch and moves the ticket to Done.
    </p>
    <Select
      label="Merge method"
      bind:value={method}
      options={[
        { value: 'squash', label: 'Squash and merge' },
        { value: 'merge', label: 'Merge commit' },
        { value: 'rebase', label: 'Rebase and merge' },
      ]}
    />
    {#if refused}
      <p class="msg" role="alert" data-testid="merge-refused">
        <Icon name="triangle-alert" size={14} />
        {refused}
      </p>
    {/if}
  </div>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    {#if refused && url}
      <Button onclick={() => void openExternal({ url }).catch((err) => toasts.error(err, 'Open in browser'))}
        >Open in browser</Button
      >
    {/if}
    <Button variant="primary" loading={busy} onclick={() => void arm()}>Merge when ready</Button>
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
