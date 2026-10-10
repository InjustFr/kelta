<script lang="ts">
  import type { ProjectId, TicketItem } from '$lib/gen';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Button, Menu, currentPlatform, formatChord } from '$lib/ui';

  import { ticketCtx } from '../plugins/actions';
  import PluginActions from '../plugins/PluginActions.svelte';
  import { blockedReason, MORE_CHORD, TICKET_ACTIONS, type TicketAction } from './caps';
  import { workflows } from './workflow.svelte';

  // Ticket detail action bar (TICKETS.md T3): the primary action, one row of quiet secondary actions,
  // and More for the rest. Keys show in tooltips and in the menu; every key works without the bar.
  // An action the tracker (or the ticket) cannot do stays visible, off, with the reason as its tooltip.
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
  /** The row; the rest of TICKET_ACTIONS goes under More. */
  const ROW: readonly TicketAction[] = ['start', 'move', 'pr', 'assign', 'comment'];
  const mac = currentPlatform() === 'macos';
  const chordText = (c: string): string => formatChord(c).join(mac ? '' : '+');

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
      chord: a.key === 'A' ? 'shift+a' : a.key,
    })),
  );
  const row = $derived(actions.filter((a) => ROW.includes(a.id)));
  const more = $derived(actions.filter((a) => !ROW.includes(a.id)));

  let moreBtn = $state<HTMLElement>();
  let menu = $state<{ x: number; y: number } | null>(null);

  /** Opens the More menu under its button (also from the detail's Mod+. key). */
  export function openMore(): void {
    const r = moreBtn?.getBoundingClientRect();
    menu = { x: r?.left ?? 0, y: (r?.bottom ?? 0) + 2 };
  }
</script>

<div class="bar" role="toolbar" aria-label="Ticket actions">
  {#each row as a (a.id)}
    <Button
      size="sm"
      variant={a.id === 'start' ? 'primary' : 'ghost'}
      class={a.reason ? 'off' : ''}
      aria-label={a.label}
      aria-keyshortcuts={a.key === 'A' ? 'Shift+A' : a.key}
      aria-disabled={a.reason ? 'true' : undefined}
      title={a.reason ?? `${a.label} (${chordText(a.chord)})`}
      data-action={a.id}
      onclick={() => {
        if (!a.reason) onrun(a.id);
      }}
    >
      {a.label}
    </Button>
  {/each}
  <span class="more" bind:this={moreBtn}>
    <Button
      size="sm"
      variant="ghost"
      icon="ellipsis"
      aria-haspopup="menu"
      aria-expanded={menu !== null}
      aria-keyshortcuts={mac ? 'Meta+.' : 'Control+Shift+.'}
      title={`More actions (${chordText(MORE_CHORD)})`}
      onclick={openMore}>More</Button
    >
  </span>
  <PluginActions kind="ticket" ctx={pluginCtx} />
</div>
{#if menu}
  <Menu
    items={more.map((a) => ({
      id: a.id,
      label: a.label,
      kbd: a.chord,
      key: a.key,
      disabled: a.reason !== null,
      title: a.reason ?? undefined,
    }))}
    x={menu.x}
    y={menu.y}
    label="More actions"
    onselect={(id) => onrun(id as TicketAction)}
    onclose={() => {
      menu = null;
      moreBtn?.querySelector('button')?.focus();
    }}
  />
{/if}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-1) var(--k-space-2);
  }

  /* The primary action stands apart from the quiet row. */
  .bar :global(.k-button.primary) {
    margin-right: var(--k-space-3);
  }

  .more {
    display: inline-flex;
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
