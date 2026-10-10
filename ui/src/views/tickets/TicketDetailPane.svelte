<script lang="ts">
  import { untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { openExternal, trackerAssign, trackerComment } from '$lib/ipc/commands';
  import { projects, tickets, toasts, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import {
    Badge,
    Button,
    EmptyState,
    ErrorState,
    HtmlContent,
    Icon,
    IconButton,
    Menu,
    TextInput,
    relativeTime,
    type MenuItem,
  } from '$lib/ui';

  import { isAuthError, statusTone } from '../work/common';
  import { openContent } from '../work/nav';
  import Loading from '../work/shared/Loading.svelte';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { selectTicket } from '../work/selection.svelte';
  import { startWorkOnTicket } from '../work/startWork';
  import MoveDialogs from './MoveDialogs.svelte';
  import { MoveController } from './move.svelte';

  let { projectId, content, focused }: PaneProps<'ticket_detail'> = $props();

  const move = new MoveController();
  const ref = $derived(content.ticket);
  const slot = $derived(tickets.details[ticketKey(ref)]);
  const detail = $derived(slot?.data ?? null);
  const ticket = $derived(detail?.ticket ?? null);
  const transitions = $derived(tickets.transitions[ticketKey(ref)]?.data ?? []);
  const workItem = $derived(work.forTicket(ref));

  $effect(() => {
    const r = ref;
    untrack(() => {
      void tickets.loadDetail(r);
      void tickets.loadTransitions(r);
    });
  });

  $effect(() => {
    if (!focused) return;
    selectTicket(ref, projectId);
    return () => selectTicket(null, null);
  });

  let root = $state<HTMLDivElement>();
  let commentBox = $state<HTMLDivElement>();
  let comment = $state('');
  let posting = $state(false);
  let menu = $state<{ x: number; y: number } | null>(null);
  let statusBtn = $state<HTMLElement>();

  $effect(() => {
    if (focused && root && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
  });

  function refresh(): void {
    void tickets.loadDetail(ref);
    void tickets.loadTransitions(ref);
  }

  async function openMoveMenu(): Promise<void> {
    if (transitions.length === 0) await tickets.loadTransitions(ref);
    const r = statusBtn?.getBoundingClientRect();
    menu = { x: r?.left ?? 40, y: (r?.bottom ?? 40) + 2 };
  }

  const menuItems = $derived<MenuItem[]>(
    transitions.map((t) => ({
      id: t.id,
      label: t.name === t.to.name ? t.name : `${t.name} → ${t.to.name}`,
      disabled: ticket !== null && t.to.name === ticket.status.name,
    })),
  );

  function menuSelect(id: string): void {
    const t = transitions.find((x) => x.id === id);
    if (t && ticket) void move.moveViaTransition(ticket, t);
  }

  async function assign(who: 'me' | 'none'): Promise<void> {
    try {
      tickets.patch(await trackerAssign({ ticket: ref, assignee: { kind: who } }));
      toasts.info(who === 'me' ? `${ref.key} assigned to you` : `${ref.key} unassigned`);
    } catch (err) {
      toasts.error(err, `Assigning ${ref.key}`);
    }
  }

  async function postComment(): Promise<void> {
    if (comment.trim() === '' || posting) return;
    posting = true;
    try {
      await trackerComment({ ticket: ref, markdown: comment });
      comment = '';
      toasts.info(`Comment added to ${ref.key}`);
      void tickets.loadDetail(ref);
    } catch (err) {
      toasts.error(err, `Commenting on ${ref.key}`);
    } finally {
      posting = false;
    }
  }

  function browse(): void {
    if (ticket) openExternal({ url: ticket.url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  function commentKeys(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void postComment();
    } else if (e.key === 'Escape') {
      e.stopPropagation();
      root?.focus();
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if ((e.target as HTMLElement).closest('input, textarea, select, [role="dialog"], [role="menu"]')) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    switch (e.key) {
      case 'm':
        void openMoveMenu();
        break;
      case 'a':
        void assign('me');
        break;
      case 'A':
        void assign('none');
        break;
      case 'c':
        commentBox?.querySelector('textarea')?.focus();
        break;
      case 'o':
        browse();
        break;
      case 's':
        void startWorkOnTicket(ref, projectId);
        break;
      case 'R':
        refresh();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  const projectName = $derived(projects.byId(projectId)?.name ?? projectId);
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="pane k-detail"
  data-testid="ticket-detail"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={`Ticket ${ref.key}`}
  {onkeydown}
>
  {#if slot?.loading && !detail}
    <Loading label="Loading ticket" />
  {:else if !detail && slot?.error}
    {#if slot.error.code === 'not_found'}
      <EmptyState icon="ticket" title={`${ref.key} was not found`} body="It may have been deleted or moved.">
        {#snippet actions()}<Button onclick={refresh}>Retry</Button>{/snippet}
      </EmptyState>
    {:else}
      <ErrorState error={slot.error} title={`Could not load ${ref.key}`} onretry={refresh}>
        {#snippet actions()}
          <Button onclick={() => void dispatch('settings.open', { section: 'accounts' })}>
            {isAuthError(slot.error) ? 'Re-authenticate' : 'Open account settings'}
          </Button>
        {/snippet}
      </ErrorState>
    {/if}
  {:else if detail && ticket}
    <StateBanner
      stale={slot?.stale ?? false}
      fetchedAt={slot?.fetchedAt}
      error={slot?.error}
      onretry={refresh}
    />
    <header class="head">
      <div class="line">
        <span class="key">{ref.key}</span>
        {#if detail.parent}
          <button
            type="button"
            class="link"
            onclick={() =>
              detail.parent && void openContent(projectId, { kind: 'ticket_detail', ticket: detail.parent })}
          >
            parent {detail.parent.key}
          </button>
        {/if}
        {#if workItem}<Badge tone="accent">local work</Badge>{/if}
      </div>
      <h1>{ticket.title}</h1>
      <div class="line actions">
        <span class="status" bind:this={statusBtn}>
          <Button aria-haspopup="menu" title="Move to… (m)" onclick={() => void openMoveMenu()}>
            <Badge tone={statusTone(ticket.status.category)}>{ticket.status.name}</Badge>
            <Icon name="chevron-down" size={14} />
          </Button>
        </span>
        <Button variant="primary" icon="play" onclick={() => void startWorkOnTicket(ref, projectId)}>
          {workItem ? 'Resume work' : 'Start work'}
        </Button>
        <Button icon="user" onclick={() => void assign('me')}>Assign me</Button>
        {#if ticket.assignee}<Button variant="ghost" onclick={() => void assign('none')}>Unassign</Button
          >{/if}
        <Button variant="ghost" icon="external-link" onclick={browse}>Open in browser</Button>
        <IconButton icon="refresh-cw" label="Refresh" onclick={refresh} />
      </div>
      <dl class="k-meta">
        <dt>Assignee</dt>
        <dd>{ticket.assignee?.name ?? 'Unassigned'}</dd>
        <dt>Priority</dt>
        <dd>{ticket.priority ?? 'None'}</dd>
        {#if ticket.kind}<dt>Type</dt>
          <dd>{ticket.kind}</dd>{/if}
        <dt>Updated</dt>
        <dd class="k-num">{relativeTime(Date.parse(ticket.updated_at))}</dd>
        <dt>Project</dt>
        <dd>{projectName}</dd>
        {#if ticket.labels.length > 0}<dt>Labels</dt>
          <dd>
            {#each ticket.labels as l (l)}<Badge>{l}</Badge>{/each}
          </dd>{/if}
      </dl>
    </header>

    <div class="scroll">
      <section aria-label="Description">
        {#if detail.body_html.trim() === ''}
          <p class="muted">No description.</p>
        {:else}
          <HtmlContent html={detail.body_html} onerror={(e) => toasts.error(e, 'Open link')} class="body" />
        {/if}
      </section>

      <section aria-label="Comments">
        <h2>Comments <span class="muted k-num">{detail.comments.length}</span></h2>
        {#each detail.comments as c, i (i)}
          <article class="comment">
            <header>
              <strong>{c.author.name}</strong>
              <span class="muted">{relativeTime(Date.parse(c.created_at))}</span>
            </header>
            <HtmlContent html={c.body_html} onerror={(e) => toasts.error(e, 'Open link')} />
          </article>
        {:else}
          <p class="muted">No comments yet.</p>
        {/each}
        <div bind:this={commentBox} class="compose">
          <TextInput
            bind:value={comment}
            label="Add a comment (Markdown, Ctrl/Cmd+Enter to send)"
            multiline
            rows={4}
            onkeydown={commentKeys}
          />
          <Button
            variant="primary"
            loading={posting}
            disabled={comment.trim() === ''}
            onclick={() => void postComment()}
          >
            Comment
          </Button>
        </div>
      </section>
    </div>
  {:else}
    <Loading label="Loading ticket" />
  {/if}
</div>

{#if menu}
  <Menu
    items={menuItems}
    x={menu.x}
    y={menu.y}
    label="Move to"
    onselect={menuSelect}
    onclose={() => {
      menu = null;
      root?.focus();
    }}
  />
{/if}
<MoveDialogs {move} />

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    outline: none;
    background: var(--k-well);
    color: var(--k-fg);
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    padding: var(--k-space-5) var(--k-space-5) var(--k-space-4);
  }

  h1 {
    margin: 0;
    max-width: var(--k-measure);
    font-size: var(--k-font-size-xl);
    font-weight: var(--k-weight-strong);
    line-height: 1.28;
  }

  h2 {
    margin: var(--k-space-5) 0 var(--k-space-3);
    font-size: var(--k-font-size);
    font-weight: var(--k-weight-strong);
  }

  .line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--k-space-3);
  }

  .key {
    font-family: var(--k-font-mono);
    color: var(--k-fg-muted);
  }

  .muted {
    color: var(--k-fg-subtle);
    font-weight: 400;
  }

  .link,
  .status {
    display: inline-flex;
  }

  .status :global(.label) {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 var(--k-space-5) var(--k-space-5);
  }

  .comment {
    max-width: var(--k-measure);
    padding: var(--k-space-3) 0;
  }

  .comment + .comment {
    border-top: 1px solid var(--k-border);
  }

  .comment header {
    display: flex;
    gap: var(--k-space-3);
    margin-bottom: var(--k-space-2);
  }

  .comment strong {
    font-weight: var(--k-weight-strong);
  }

  .compose {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-3);
    max-width: var(--k-measure);
    margin-top: var(--k-space-4);
  }

  .compose :global(.k-field) {
    align-self: stretch;
  }
</style>
