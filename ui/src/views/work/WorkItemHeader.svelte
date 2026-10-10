<script lang="ts">
  import { untrack } from 'svelte';

  import type { TabHeaderProps } from '$app/registry';
  import type { GitStatus } from '$lib/gen';
  import { openExternal, workRetryStep, workStatus } from '$lib/ipc/commands';
  import { tickets, toasts, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import { Badge, Button, Icon, Menu, type MenuItem } from '$lib/ui';

  import MoveDialogs from '../tickets/MoveDialogs.svelte';
  import { MoveController } from '../tickets/move.svelte';
  import { statusTone, type Tone } from './common';
  import CreatePrDialog from './CreatePrDialog.svelte';
  import FinishDialog from './FinishDialog.svelte';
  import { openContent } from './nav';

  let { projectId, workItemId }: TabHeaderProps = $props();

  const move = new MoveController();
  const item = $derived(work.get(workItemId));
  const ticketRef = $derived(item?.ticket ?? null);
  const detail = $derived(ticketRef ? tickets.details[ticketKey(ticketRef)]?.data : null);
  const transitions = $derived(ticketRef ? (tickets.transitions[ticketKey(ticketRef)]?.data ?? []) : []);

  let git = $state<GitStatus | null>(null);
  let gitError = $state(false);
  let creating = $state(false);
  let finishing = $state(false);
  let menu = $state<{ x: number; y: number } | null>(null);
  let statusBtn = $state<HTMLElement>();
  let retrying = $state(false);

  async function refreshGit(): Promise<void> {
    if (!item || item.state.kind === 'finished') return;
    try {
      git = await workStatus({ id: workItemId });
      gitError = false;
    } catch {
      gitError = true;
    }
  }

  // Ahead/behind are read when the tab gains focus (no polling): on mount, on window focus and
  // whenever the work item changes state.
  $effect(() => {
    void item?.state.kind;
    untrack(() => void refreshGit());
  });

  $effect(() => {
    const ref = ticketRef;
    if (ref) {
      untrack(() => {
        void tickets.loadDetail(ref);
        void tickets.loadTransitions(ref);
      });
    }
  });

  const stateInfo = $derived.by((): { label: string; tone: Tone } => {
    switch (item?.state.kind) {
      case 'planned':
        return { label: 'Planned', tone: 'neutral' };
      case 'starting':
        return { label: 'Starting', tone: 'info' };
      case 'active':
        return { label: 'Active', tone: 'info' };
      case 'pr_open':
        return { label: 'PR open', tone: 'accent' };
      case 'finished':
        return { label: 'Finished', tone: 'ok' };
      case 'failed':
        return { label: `Failed: ${item.state.step}`, tone: 'danger' };
      default:
        return { label: 'Unknown', tone: 'neutral' };
    }
  });

  const menuItems = $derived<MenuItem[]>(
    transitions.map((t) => ({
      id: t.id,
      label: t.name === t.to.name ? t.name : `${t.name} → ${t.to.name}`,
      disabled: detail?.ticket.status.name === t.to.name,
    })),
  );

  function openMenu(): void {
    const r = statusBtn?.getBoundingClientRect();
    menu = { x: r?.left ?? 40, y: (r?.bottom ?? 40) + 2 };
  }

  function menuSelect(id: string): void {
    const t = transitions.find((x) => x.id === id);
    if (t && detail) void move.moveViaTransition(detail.ticket, t);
  }

  function browse(url: string | null | undefined): void {
    if (url) openExternal({ url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  async function retryFailed(): Promise<void> {
    if (item?.state.kind !== 'failed') return;
    retrying = true;
    try {
      work.upsert(await workRetryStep({ id: item.id, step: item.state.step }));
    } catch (err) {
      toasts.error(err, 'Retry');
    } finally {
      retrying = false;
    }
  }
</script>

<svelte:window onfocus={() => void refreshGit()} />

{#if item}
  <div class="header" role="toolbar" aria-label="Work item" data-testid="work-header">
    {#if ticketRef}
      <span class="key">{ticketRef.key}</span>
      <button
        type="button"
        class="status"
        bind:this={statusBtn}
        aria-haspopup="menu"
        title="Move to…"
        disabled={!detail}
        onclick={openMenu}
      >
        <Badge tone={statusTone(detail?.ticket.status.category ?? 'unknown')}>
          {detail?.ticket.status.name ?? '…'}
        </Badge>
        <Icon name="chevron-down" size={12} />
      </button>
    {:else if item.review}
      <span class="key">{item.review.repo}#{item.review.number}</span>
    {/if}
    <Badge tone={stateInfo.tone} title="Work item state">{stateInfo.label}</Badge>
    <span class="branch" title={item.worktree}>
      <Icon name="git-branch" size={12} />
      <code>{item.branch}</code>
    </span>
    {#if git}
      <span
        class="git"
        title={`${git.ahead} ahead, ${git.behind} behind ${item.base}${git.dirty ? ', uncommitted changes' : ''}${git.unpushed ? ', unpushed commits' : ''}`}
      >
        <span data-testid="ahead">↑{git.ahead}</span>
        <span data-testid="behind">↓{git.behind}</span>
        <!-- A mono "*" like a modified buffer, not a dot: saturated dots are reserved for lamps. -->
        {#if git.dirty}<span aria-label="Uncommitted changes">*</span>{/if}
      </span>
    {:else if gitError}
      <button type="button" class="link" onclick={() => void refreshGit()}>status unavailable, retry</button>
    {/if}
    <span class="spacer"></span>
    {#if item.state.kind === 'failed'}
      <Button size="sm" loading={retrying} onclick={() => void retryFailed()}>Retry {item.state.step}</Button>
    {/if}
    {#if item.state.kind !== 'finished'}
      {#if item.review}
        {@const ref = item.review}
        <Button
          size="sm"
          variant="primary"
          icon="git-pull-request"
          onclick={() => void openContent(projectId, { kind: 'review_detail', review: ref })}
        >
          Open review
        </Button>
      {:else if item.pr_url}
        <Button size="sm" icon="git-pull-request" onclick={() => browse(item.pr_url)}>Open PR</Button>
      {:else}
        <Button size="sm" icon="git-pull-request" onclick={() => (creating = true)}>Create PR</Button>
      {/if}
    {/if}
    {#if ticketRef}
      <Button
        size="sm"
        variant="ghost"
        icon="ticket"
        onclick={() => ticketRef && void openContent(projectId, { kind: 'ticket_detail', ticket: ticketRef })}
      >
        Ticket
      </Button>
      <Button
        size="sm"
        variant="ghost"
        icon="external-link"
        onclick={() => browse(detail?.ticket.url)}
        disabled={!detail}
      >
        Browser
      </Button>
    {:else if item.review}
      <Button
        size="sm"
        variant="ghost"
        icon="external-link"
        onclick={() => browse(item.pr_url)}
        disabled={!item.pr_url}
      >
        Browser
      </Button>
    {/if}
    {#if item.state.kind !== 'finished'}
      <Button size="sm" onclick={() => (finishing = true)}>Finish</Button>
    {/if}
  </div>
{:else}
  <div class="header" data-testid="work-header">
    <span class="muted">Work item not found</span>
  </div>
{/if}

{#if menu}
  <Menu
    items={menuItems}
    x={menu.x}
    y={menu.y}
    label="Move to"
    onselect={menuSelect}
    onclose={() => (menu = null)}
  />
{/if}
{#if creating && item}
  <CreatePrDialog {item} onclose={() => (creating = false)} />
{/if}
{#if finishing && item}
  <FinishDialog {item} onclose={() => (finishing = false)} />
{/if}
<MoveDialogs {move} />

<style>
  .header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--k-space-2) var(--k-space-4);
    min-height: var(--k-tabbar-height);
    padding: var(--k-space-1) var(--k-space-3);
    background: var(--k-bezel-raised);
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-sm);
  }

  .key {
    font-family: var(--k-font-mono);
    color: var(--k-fg);
  }

  .branch {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    color: var(--k-fg-muted);
  }

  .git {
    display: inline-flex;
    gap: var(--k-space-2);
    font-family: var(--k-font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-muted);
  }

  .spacer {
    flex: 1;
  }

  .status,
  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-fg-muted);
    font: inherit;
    cursor: pointer;
  }

  .link {
    color: var(--k-accent);
  }

  .muted {
    color: var(--k-fg-subtle);
  }
</style>
