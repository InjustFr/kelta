<script lang="ts">
  import { untrack } from 'svelte';

  import { t, tn } from '$lib/i18n/index.svelte';

  import type { WorkItem } from '$lib/gen';
  import { openExternal } from '$lib/ipc/commands';
  import { settings, tickets, toasts } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Button, Icon } from '$lib/ui';

  import { readCriteria } from '../../tickets/spec';
  import StatusChip from '../../tickets/StatusChip.svelte';
  import StatusPicker from '../../tickets/StatusPicker.svelte';
  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  // The context bar loads the detail too; the panel does not rely on it.
  $effect(() => {
    const ref = item.ticket;
    if (ref) untrack(() => void tickets.loadDetail(ref));
  });

  const detail = $derived(item.ticket ? tickets.details[ticketKey(item.ticket)]?.data : null);
  const source = $derived(
    settings.value()?.sources.find((s) => s.id === item.ticket_source)?.label || item.ticket?.account,
  );
  const criteria = $derived(detail ? readCriteria(detail.body_html) : null);
  const checked = $derived(criteria?.items.filter((c) => c.checked).length ?? 0);
  const summary = $derived(
    [
      source,
      detail?.ticket.status.name ?? '…',
      criteria?.items.length
        ? tn('{checked} of {n} criterion checked', '{checked} of {n} criteria checked', criteria.items.length, {
            checked,
          })
        : null,
    ]
      .filter(Boolean)
      .join(', '),
  );

  let picking = $state<HTMLElement | null>(null);
</script>

{#if item.ticket}
  <Section title={t('Ticket')} {summary} {open} testid="story-ticket">
    {#if detail}
      <div class="head">
        <span class="key num">{detail.ticket.ref.key}</span>
        <span class="src">{source}</span>
        <Button
          size="sm"
          variant="secondary"
          aria-haspopup="menu"
          title={t('Move to…')}
          data-testid="story-ticket-status"
          onclick={(e: MouseEvent) => (picking = e.currentTarget as HTMLElement)}
        >
          <StatusChip status={detail.ticket.status} source={item.ticket_source} />
          <Icon name="chevron-down" size={14} />
        </Button>
      </div>
      <div class="title">{detail.ticket.title}</div>
      {#if criteria?.items.length}
        <ul class="criteria" aria-label={t('Acceptance criteria')} data-testid="story-criteria">
          {#each criteria.items as c, i (i)}
            <li>
              <input type="checkbox" checked={c.checked} disabled aria-label={c.text} />
              <span class:done={c.checked}>{c.text}</span>
            </li>
          {/each}
        </ul>
      {:else}
        {#if criteria?.intro}<p class="intro">{criteria.intro}</p>{/if}
        <button
          type="button"
          class="k-link"
          onclick={() =>
            void openExternal({ url: detail.ticket.url }).catch((err) =>
              toasts.error(err, t('Open in browser')),
            )}>{t('Open in tracker')}</button
        >
      {/if}
    {:else}
      …
    {/if}
  </Section>
  {#if picking && detail}
    <StatusPicker tickets={[detail.ticket]} anchor={picking} onclose={() => (picking = null)} />
  {/if}
{:else}
  <Section title={t('Ticket')} summary={t('No ticket')} {open} testid="story-ticket" />
{/if}

<style>
  .head {
    display: flex;
    gap: var(--k-space-2);
    align-items: center;
  }

  .key {
    font-weight: 600;
  }

  .src {
    flex: 1;
    min-width: 0;
    color: var(--k-fg-muted);
  }

  .title {
    margin: var(--k-space-2) 0;
  }

  .criteria {
    padding: 0;
    margin: 0;
    list-style: none;
  }

  .criteria li {
    display: flex;
    gap: var(--k-space-2);
    align-items: baseline;
    padding: 2px 0;
  }

  .done {
    color: var(--k-fg-muted);
  }

  .intro {
    margin: 0 0 var(--k-space-2);
    color: var(--k-fg-muted);
  }
</style>
