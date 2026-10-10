<script lang="ts">
  import { untrack } from 'svelte';

  import type { WorkItem } from '$lib/gen';
  import { workFinishMerged, workStatus } from '$lib/ipc/commands';
  import { toasts, work } from '$lib/stores';
  import { Button, Dialog, Spinner } from '$lib/ui';

  import { prNumber } from './ship';

  // "Finish all merged" (FLOW §4.6): the only bulk destructive action. Lists merged items with a
  // clean worktree; dirty ones and those waiting for a Done choice are listed as skipped. The
  // backend finishes only the listed ids and re-checks each one.

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  interface Row {
    item: WorkItem;
    skip: string | null;
  }

  let rows = $state<Row[] | null>(null);
  let busy = $state(false);
  const ready = $derived(rows?.filter((r) => !r.skip) ?? []);
  const skipped = $derived(rows?.filter((r) => r.skip) ?? []);

  $effect(() => untrack(() => void load()));

  async function load(): Promise<void> {
    const merged = work.all.filter((w) => w.state.kind === 'merged');
    rows = await Promise.all(
      merged.map(async (item): Promise<Row> => {
        if (item.state.kind === 'merged' && item.state.detail) return { item, skip: item.state.detail };
        try {
          const g = await workStatus({ id: item.id });
          const skip = g.dirty ? 'uncommitted changes' : g.unpushed ? 'unpushed commits' : null;
          return { item, skip };
        } catch {
          return { item, skip: 'worktree status unavailable' };
        }
      }),
    );
  }

  async function finish(): Promise<void> {
    if (busy || ready.length === 0) return;
    busy = true;
    try {
      const report = await workFinishMerged({ ids: ready.map((r) => r.item.id) });
      for (const w of report.finished) work.upsert(w);
      const n = report.finished.length;
      const k = skipped.length + report.skipped.length;
      const more = k ? `, ${k} skipped` : '';
      toasts.info(`Finished ${n} merged work item${n === 1 ? '' : 's'}${more}`);
      onclose();
    } catch (err) {
      toasts.error(err, 'Finish all merged');
    } finally {
      busy = false;
    }
  }

  const name = (w: WorkItem): string => w.ticket?.key ?? w.branch;
</script>

<Dialog title="Finish all merged" tone="danger" width={520} {onclose}>
  <div class="body" data-testid="finish-merged">
    {#if rows === null}
      <p class="hint"><Spinner size={12} /> Checking worktrees…</p>
    {:else if rows.length === 0}
      <p class="hint">No merged work items.</p>
    {:else}
      {#if ready.length > 0}
        <p class="hint">Stops their sessions, removes their worktrees and deletes their local branches.</p>
        <ul aria-label="To finish">
          {#each ready as r (r.item.id)}
            <li>
              <span class="key">{name(r.item)}</span> <span class="meta">{prNumber(r.item.pr_url)}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if skipped.length > 0}
        <h3>Skipped</h3>
        <ul aria-label="Skipped">
          {#each skipped as r (r.item.id)}
            <li><span class="key">{name(r.item)}</span> <span class="meta">{r.skip}</span></li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button variant="primary" loading={busy} disabled={ready.length === 0} onclick={() => void finish()}>
      Finish {ready.length}
    </Button>
  {/snippet}
</Dialog>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  .hint {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: 0;
    color: var(--k-fg-muted);
  }

  h3 {
    margin: var(--k-space-2) 0 0;
    font-size: var(--k-font-size-sm);
    font-weight: 600;
    color: var(--k-fg-muted);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    justify-content: space-between;
    gap: var(--k-space-3);
    padding: 2px 0;
  }

  .key {
    font-family: var(--k-font-mono);
  }

  .meta {
    color: var(--k-fg-muted);
    font-variant-numeric: tabular-nums;
  }
</style>
