<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import type { TicketItem } from '$lib/gen';
  import { dispatch } from '$lib/actions';
  import { tickets } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Button, EmptyState, ErrorState } from '$lib/ui';

  import { nextUp } from '../inbox/nextUp.svelte';
  import { isAuthError } from '../work/common';
  import Loading from '../work/shared/Loading.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import TicketDetail from './TicketDetail.svelte';

  // Standalone ticket pane: loads the ticket, then hands it to the embeddable `TicketDetail`.
  let { projectId, content, focused }: PaneProps<'ticket_detail'> = $props();

  const ref = $derived(content.ticket);
  const slot = $derived(tickets.details[ticketKey(ref)]);
  const detail = $derived(slot?.data ?? null);
  /** A detail opened on its own has no list item: build one from `tracker_get` (it carries prs and caps). */
  const item = $derived<TicketItem | null>(
    detail && {
      ticket: detail.ticket,
      project_ids: [projectId],
      work_item_id: null,
      view_ids: [],
      prs: detail.prs,
      caps: detail.caps,
    },
  );

  let host = $state<HTMLDivElement>();

  $effect(() => {
    const r = ref;
    untrack(() => {
      void tickets.loadDetail(r);
      // Opened: no longer New (#145).
      nextUp.markSeen(r);
    });
  });

  $effect(() => {
    if (!focused) return;
    selectTicket(ref, projectId);
    return () => selectTicket(null, null);
  });

  $effect(() => {
    const el = item && host?.querySelector<HTMLElement>('[data-testid="ticket-detail"]');
    if (focused && el && !el.contains(document.activeElement)) el.focus({ preventScroll: true });
  });
</script>

<div class="pane" bind:this={host}>
  {#if item}
    <TicketDetail {item} {projectId} />
  {:else if slot?.error && !slot.loading}
    {#if slot.error.code === 'not_found'}
      <EmptyState icon="ticket" title={`${ref.key} was not found`} body="It may have been deleted or moved.">
        {#snippet actions()}<Button onclick={() => void tickets.loadDetail(ref)}>Retry</Button>{/snippet}
      </EmptyState>
    {:else}
      <ErrorState
        error={slot.error}
        title={`Could not load ${ref.key}`}
        onretry={() => void tickets.loadDetail(ref)}
      >
        {#snippet actions()}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
            {isAuthError(slot.error) ? 'Re-authenticate' : 'Open account settings'}
          </Button>
        {/snippet}
      </ErrorState>
    {/if}
  {:else}
    <Loading label="Loading ticket" />
  {/if}
</div>

<style>
  .pane {
    height: 100%;
    min-height: 0;
    background: var(--k-well);
  }
</style>
