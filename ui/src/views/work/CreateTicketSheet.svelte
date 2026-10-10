<script lang="ts">
  // Create ticket (FLOW §4.3 step 5): title and body prefilled from the scratch item, filed in a tracker
  // source of its project, then linked like Link to ticket.
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type { WorkItemId } from '$lib/gen';
  import { workCreateTicket, workPrDraft, workStatus } from '$lib/ipc/commands';
  import { projects, toasts, work } from '$lib/stores';
  import { Button, Dialog, Select, TextInput, Toggle } from '$lib/ui';

  import { ticketBody } from './common';

  interface Props extends SheetProps {
    id: WorkItemId;
  }

  let { id, onclose }: Props = $props();

  const item = $derived(work.get(id));
  const views = $derived((item && projects.byId(item.project_id)?.tracker?.views) ?? []);
  let viewId = $state('');
  let title = $state('');
  let body = $state('');
  let apply = $state(true);
  let loading = $state(true);
  let busy = $state(false);

  $effect(() => {
    viewId ||= views[0]?.id ?? '';
  });

  $effect(() => {
    const branch = item?.branch ?? '';
    untrack(() => void load(id, branch));
  });

  async function load(id: WorkItemId, branch: string): Promise<void> {
    const [d, g] = await Promise.allSettled([workPrDraft({ id }), workStatus({ id })]);
    const draft = d.status === 'fulfilled' ? d.value : null;
    // Keep what was typed while the defaults loaded.
    title ||= draft?.title ?? item?.title ?? '';
    body ||= ticketBody(draft?.body ?? '', branch, g.status === 'fulfilled' ? g.value : null);
    loading = false;
  }

  async function create(): Promise<void> {
    if (busy || !viewId || !title.trim()) return;
    busy = true;
    try {
      const next = work.upsert(
        await workCreateTicket({ id, view_id: viewId, title, body_md: body, apply_side_effects: apply }),
      );
      toasts.info(`Created and linked ${next.ticket?.key ?? 'the ticket'}`);
      onclose();
    } catch (err) {
      toasts.error(err, 'Create ticket');
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void create();
    }
  }
</script>

<svelte:window {onkeydown} />

<Dialog title="Create ticket" width={520} {onclose}>
  <div class="form" data-testid="create-ticket">
    {#if views.length === 0}
      <p class="muted" role="alert">This project has no tracker. Add one in its settings first.</p>
    {:else if views.length > 1}
      <Select label="In" bind:value={viewId} options={views.map((v) => ({ value: v.id, label: v.label }))} />
    {/if}
    <TextInput label="Title" bind:value={title} />
    <TextInput label="Description" bind:value={body} multiline rows={6} />
    <p class="muted">The branch <code>{item?.branch}</code> stays as it is.</p>
    <Toggle
      label={item?.pr_url
        ? 'Assign me, move the ticket to In Progress then In Review, and comment the PR link'
        : 'Assign me and move the ticket to In Progress'}
      bind:checked={apply}
    />
  </div>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button
      variant="primary"
      loading={busy}
      disabled={loading || !viewId || !title.trim()}
      onclick={() => void create()}
      data-testid="create-ticket-submit">Create ticket</Button
    >
  {/snippet}
</Dialog>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  p {
    margin: 0;
  }

  .muted {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }
</style>
