<script lang="ts">
  import type { ProjectId, TicketItem } from '$lib/gen';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Button } from '$lib/ui';

  import { ticketCtx } from '../plugins/actions';
  import PluginActions from '../plugins/PluginActions.svelte';
  import { blockedReason, TICKET_ACTIONS, type TicketAction } from './caps';
  import { workflows } from './workflow.svelte';

  // Ticket detail action bar (TICKETS.md T3): one button per action with its key; an action the
  // tracker (or the ticket) cannot do stays visible, off, with the reason as its tooltip.
  interface Props {
    item: TicketItem;
    branch: string | null;
    /** A work item exists: Start reads Resume. */
    resume: boolean;
    onrun: (action: TicketAction) => void;
    /** The detail's project; `null` in a global view. */
    projectId?: ProjectId | null;
  }

  let { item, branch, resume, onrun, projectId = null }: Props = $props();

  /** Plugin actions run where the ticket's work runs, else in the ticket's (or the pane's) project. */
  const pluginCtx = $derived(ticketCtx(item.ticket.ref, item.work_item_id, item.project_ids, projectId));

  const LABELS: Record<TicketAction, string> = {
    start: 'Start work',
    move: 'Move',
    pr: 'Open PR',
    assign: 'Assign to me',
    unassign: 'Unassign',
    comment: 'Comment',
    refine: 'Refine with Claude',
    workflow: 'Run workflow…',
    branch: 'Copy branch',
    browser: 'Open in browser',
  };

  const actions = $derived(
    TICKET_ACTIONS.filter((a) => a.id !== 'unassign' || item.ticket.assignee).map((a) => ({
      ...a,
      label:
        a.id === 'start' && resume
          ? 'Resume work'
          : a.id === 'pr' && item.prs.length > 1
            ? 'Open PRs'
            : LABELS[a.id],
      reason: blockedReason(a.id, item, branch, workflows[ticketKey(item.ticket.ref)]),
    })),
  );
</script>

<div class="bar" role="toolbar" aria-label="Ticket actions">
  {#each actions as a (a.id)}
    <Button
      size="sm"
      variant={a.id === 'start' ? 'primary' : 'ghost'}
      class={a.reason ? 'off' : ''}
      aria-label={a.label}
      aria-keyshortcuts={a.key === 'A' ? 'Shift+A' : a.key}
      aria-disabled={a.reason ? 'true' : undefined}
      title={a.reason ?? `${a.label} (${a.key})`}
      data-action={a.id}
      chord={a.key === 'A' ? 'shift+a' : a.key}
      onclick={() => {
        if (!a.reason) onrun(a.id);
      }}
    >
      {a.label}
    </Button>
  {/each}
  <PluginActions kind="ticket" ctx={pluginCtx} />
</div>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-1) var(--k-space-2);
  }

  /* Off, not disabled: it keeps its tooltip (the reason) and stays reachable with Tab. */
  .bar :global(.k-button.off) {
    opacity: 0.55;
    cursor: default;
  }

  .bar :global(.k-button.ghost.off:hover) {
    background: transparent;
  }
</style>
