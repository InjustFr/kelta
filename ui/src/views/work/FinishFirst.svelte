<script lang="ts">
  // Finish before starting (#354): the start sheet's first step when other work is unfinished.
  import { t } from '$lib/i18n/index.svelte';
  import { projects } from '$lib/stores';
  import { Badge } from '$lib/ui';

  import type { Unfinished } from './finishFirst';
  import { goToWork, openFromNow } from './nav';

  interface Props {
    rows: Unfinished[];
    /** Called once a row's item is shown: the sheet closes. */
    ongo: () => void;
  }

  let { rows, ongo }: Props = $props();

  async function go(row: Unfinished): Promise<void> {
    if (row.item) await goToWork(row.item);
    else if (row.ticket) {
      const pid = row.ticket.project_ids[0] ?? projects.activeId;
      if (pid) await openFromNow(pid, { kind: 'ticket_detail', ticket: row.ticket.ticket.ref });
    }
    ongo();
  }
</script>

<div class="finish" data-testid="finish-first">
  <p class="note">{t('You have unfinished work. Finish it first, or start anyway.')}</p>
  <ul>
    {#each rows as row (row.item?.id ?? row.key)}
      <li>
        <button type="button" class="row" onclick={() => void go(row)}>
          <code>{row.key}</code>
          <span class="title">{row.title}</span>
          {#if row.blocked}<Badge tone="danger">{t('Blocked')}</Badge>{/if}
          <Badge>{row.phase}</Badge>
        </button>
      </li>
    {/each}
  </ul>
</div>

<style>
  .finish {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .note {
    margin: 0;
    color: var(--k-fg-muted);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    width: 100%;
    padding: var(--k-space-2);
    border: 0;
    border-radius: var(--k-radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row:hover,
  .row:focus-visible {
    background: var(--k-bg-sunken);
  }

  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
