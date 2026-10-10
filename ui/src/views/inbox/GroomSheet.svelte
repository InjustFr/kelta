<script lang="ts">
  // Grooming pass (#145, palette `Groom`): one ticket at a time across every bound tracker, mine and
  // unassigned, New ones first, triaged into my Next up list with single keys.
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type { TicketItem } from '$lib/gen';
  import { openExternal, trackerAssign } from '$lib/ipc/commands';
  import { projects, tickets, toasts, ui, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Badge, EmptyState, Kbd, Sheet } from '$lib/ui';

  import { blockedReason } from '../tickets/caps';
  import TicketDetail from '../tickets/TicketDetail.svelte';
  import { groomQueue, loadPool, nextUp, SNOOZE_DAYS, ticketPool } from './nextUp.svelte';

  let { onclose }: SheetProps = $props();

  untrack(() => void loadPool());

  const queue = $derived(
    groomQueue(
      ticketPool(),
      nextUp.entries,
      (k) => nextUp.loaded && !(k in nextUp.seen),
      (t) => work.forTicket(t.ticket.ref) !== null,
      Date.now(),
    ),
  );
  /** Followed by key: `o` clears New and moves the ticket within the queue. */
  let curKey = $state<string | null>(null);
  const at = $derived(
    Math.max(
      0,
      queue.findIndex((t) => ticketKey(t.ticket.ref) === curKey),
    ),
  );
  const cur = $derived<TicketItem | null>(queue[at] ?? null);
  const project = $derived(cur ? projects.byId(cur.project_ids[0] ?? '') : null);

  function go(i: number): void {
    const t = queue[Math.min(queue.length - 1, Math.max(0, i))];
    curKey = t ? ticketKey(t.ticket.ref) : null;
  }

  /** The ticket leaves the queue: show the one after it (or before, at the end). */
  function leave(): void {
    const next = queue[at + 1] ?? queue[at - 1];
    curKey = next ? ticketKey(next.ticket.ref) : null;
  }

  async function assign(item: TicketItem, who: 'me' | 'none'): Promise<void> {
    const reason = blockedReason(who === 'me' ? 'assign' : 'unassign', item, null);
    if (reason) return void toasts.info(reason);
    const ref = item.ticket.ref;
    try {
      tickets.patch(await trackerAssign({ ticket: ref, assignee: { kind: who } }));
      toasts.info(who === 'me' ? `${ref.key} assigned to you` : `${ref.key} unassigned`);
    } catch (err) {
      toasts.error(err, `Assigning ${ref.key}`);
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if ((e.target as HTMLElement).closest('input, textarea, select, [role="menu"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey || !cur) return;
    const ref = cur.ticket.ref;
    const pid = cur.project_ids[0] ?? null;
    switch (e.key) {
      case 'n':
      case 'N':
        leave();
        void nextUp.add(ref, pid, e.key === 'N');
        toasts.info(`${ref.key} added to Next up${e.key === 'N' ? ' (top)' : ''}`);
        break;
      case 'l':
        leave();
        void nextUp.snooze(ref, pid);
        toasts.info(`${ref.key} snoozed for ${SNOOZE_DAYS} days`);
        break;
      case 'a':
      case 'u':
        void assign(cur, e.key === 'a' ? 'me' : 'none');
        break;
      case 'm':
        ui.openSheet('tickets.move', { ticket: cur.ticket, project_id: pid });
        break;
      case 'o':
        curKey = ticketKey(ref);
        nextUp.markSeen(ref);
        openExternal({ url: cur.ticket.url }).catch((err) => toasts.error(err, 'Open in browser'));
        break;
      case 'j':
        go(at + 1);
        break;
      case 'k':
        go(at - 1);
        break;
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  const KEYS: [string, string][] = [
    ['n', 'Next up'],
    ['shift+n', 'Next up, top'],
    ['l', `Snooze ${SNOOZE_DAYS} days`],
    ['a', 'Assign me'],
    ['u', 'Unassign'],
    ['m', 'Move'],
    ['o', 'Open in browser'],
    ['j', 'Skip'],
    ['k', 'Back'],
  ];
</script>

<Sheet title="Groom" width={4000} {onclose}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions, a11y_autofocus -->
  <div
    class="groom"
    data-testid="groom"
    tabindex="0"
    role="group"
    aria-label="Groom tickets"
    autofocus
    {onkeydown}
  >
    {#if cur}
      <div class="bar">
        <span class="k-num">{at + 1} / {queue.length}</span>
        {#if nextUp.isNew(cur.ticket.ref)}<Badge tone="accent">New</Badge>{/if}
        {#if project}<span class="muted">{project.name}</span>{/if}
        <span class="muted">{cur.ticket.status.name}</span>
      </div>
      <div class="detail">
        <TicketDetail item={cur} projectId={cur.project_ids[0] ?? null} embedded />
      </div>
    {:else}
      <EmptyState
        icon="ticket"
        title="Nothing to groom"
        body="Every open ticket assigned to you or unassigned is in Next up, snoozed or started."
      />
    {/if}
    <footer class="hints">
      {#each KEYS as [k, label] (k)}<span><Kbd chord={k} /> {label}</span>{/each}
    </footer>
  </div>
</Sheet>

<style>
  .groom {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    height: 100%;
    min-height: 0;
    outline: none;
  }

  .bar {
    display: flex;
    gap: var(--k-space-3);
    align-items: center;
  }

  .detail {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .hints {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-3);
    color: var(--k-fg-muted);
  }
</style>
