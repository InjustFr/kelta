<script lang="ts">
  // Link to ticket (FLOW §4.3 step 4): ticket picker, then one confirmation with the side-effects toggle.
  import type { SheetProps } from '$app/registry';
  import type { TicketItem, WorkItemId } from '$lib/gen';
  import { workLink } from '$lib/ipc/commands';
  import { toasts, work } from '$lib/stores';
  import { Button, Dialog, Toggle } from '$lib/ui';

  import TicketPicker from './TicketPicker.svelte';

  interface Props extends SheetProps {
    id: WorkItemId;
  }

  let { id, onclose }: Props = $props();

  const item = $derived(work.get(id));
  let picked = $state<TicketItem | null>(null);
  let apply = $state(true);
  let busy = $state(false);

  async function link(): Promise<void> {
    if (!picked || busy) return;
    busy = true;
    try {
      const next = await workLink({ id, ticket: picked.ticket.ref, apply_side_effects: apply });
      work.upsert(next);
      toasts.info(`Linked ${picked.ticket.ref.key}`);
      onclose();
    } catch (err) {
      toasts.error(err, `Linking ${picked.ticket.ref.key}`);
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (picked && e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void link();
    }
  }
</script>

<svelte:window {onkeydown} />

{#if !picked}
  <TicketPicker title="Link to ticket" onpick={(hit) => (picked = hit)} {onclose} />
{:else}
  <Dialog title={`Link to ${picked.ticket.ref.key}`} {onclose}>
    <div class="body" data-testid="link-ticket">
      <p>
        <code>{picked.ticket.ref.key}</code>
        {picked.ticket.title}
      </p>
      <p class="muted">
        The branch <code>{item?.branch}</code> stays as it is.
      </p>
      <Toggle
        label={item?.pr_url
          ? 'Assign me, move the ticket to In Progress then In Review, and comment the PR link'
          : 'Assign me and move the ticket to In Progress'}
        bind:checked={apply}
      />
      {#if item?.pr_url}
        <p class="muted">The next push adds {picked.ticket.ref.key} to the PR title if it has no key yet.</p>
      {/if}
    </div>
    {#snippet actions()}
      <Button variant="ghost" onclick={() => (picked = null)}>Back</Button>
      <Button variant="primary" loading={busy} onclick={() => void link()} data-testid="link-ticket-submit"
        >Link ticket</Button
      >
    {/snippet}
  </Dialog>
{/if}

<style>
  .body {
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
