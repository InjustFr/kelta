<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
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
  /** A detail opened on its own has no list item: build one from `tracker_get` (it carries prs and caps),
   *  with the sources of a loaded list that has the ticket (its status colour follows them). */
  const listed = $derived(
    Object.values(tickets.lists)
      .flatMap((l) => l.data?.items ?? [])
      .find((it) => ticketKey(it.ticket.ref) === ticketKey(ref)),
  );
  const item = $derived<TicketItem | null>(
    detail && {
      ticket: detail.ticket,
      project_ids: [],
      work_item_id: null,
      view_ids: [],
      source_ids: listed?.source_ids ?? [],
      prs: detail.prs,
      caps: detail.caps,
      waiting: [],
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
    // A standalone ticket has no project of its own: start work must not default to the pane's (SPEC §3.1).
    selectTicket(ref, null);
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
      <EmptyState
        icon="ticket"
        title={t('{key} was not found', { key: ref.key })}
        body={t('It may have been deleted or moved.')}
      >
        {#snippet actions()}<Button onclick={() => void tickets.loadDetail(ref)}>{t('Retry')}</Button
          >{/snippet}
      </EmptyState>
    {:else}
      <ErrorState
        error={slot.error}
        title={t('Could not load {name}', { name: ref.key })}
        onretry={() => void tickets.loadDetail(ref)}
      >
        {#snippet actions()}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
            {isAuthError(slot.error) ? t('Re-authenticate') : t('Open account settings')}
          </Button>
        {/snippet}
      </ErrorState>
    {/if}
  {:else}
    <Loading label={t('Loading ticket')} />
  {/if}
</div>

<style>
  .pane {
    height: 100%;
    min-height: 0;
    background: var(--k-well);
  }
</style>
