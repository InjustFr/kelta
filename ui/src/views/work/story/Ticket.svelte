<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';

  import type { WorkItem } from '$lib/gen';
  import { tickets } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';

  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  // The context bar loads the detail; its status is the summary.
  const status = $derived(
    item.ticket ? tickets.details[ticketKey(item.ticket)]?.data?.ticket.status.name : null,
  );
</script>

<Section
  title={t('Ticket')}
  summary={item.ticket ? (status ?? '…') : t('No ticket')}
  {open}
  testid="story-ticket"
/>
