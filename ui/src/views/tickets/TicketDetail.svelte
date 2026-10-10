<script lang="ts">
  import { untrack } from 'svelte';

  import type { Assignee, CiState, ProjectId, TicketItem, TicketRef } from '$lib/gen';
  import {
    clipboardWrite,
    openExternal,
    trackerAssign,
    trackerAssignableUsers,
    trackerComment,
    trackerPriorities,
    trackerSetPriority,
  } from '$lib/ipc/commands';
  import { projects, tickets, toasts, work } from '$lib/stores';
  import { ticketKey } from '$lib/stores/tickets.svelte';
  import {
    Badge,
    Button,
    HtmlContent,
    Icon,
    Lamp,
    Menu,
    type MenuItem,
    TextInput,
    relativeTime,
  } from '$lib/ui';

  import { openContent } from '../work/nav';
  import StateBanner from '../work/shared/StateBanner.svelte';
  import { startWorkOnTicket } from '../work/startWork';
  import ActionBar from './ActionBar.svelte';
  import { blockedReason, TICKET_ACTIONS, type TicketAction } from './caps';
  import PickMenu from './PickMenu.svelte';
  import { discard, refine, refines } from './refine.svelte';
  import { ciLamp, ensureReviews, mainPr, openPr, openPrs, prLabel, prMenuItems, reviewWord } from './prs';
  import StatusChip from './StatusChip.svelte';
  import StatusPicker from './StatusPicker.svelte';

  // Ticket detail body (TICKETS.md T3): the standalone pane wraps it, the tickets list embeds it as
  // its split view. Header (key, title, status, actions) comes from the list item at once; the
  // description and comments load with `tracker_get`.
  interface Props {
    item: TicketItem;
    projectId: ProjectId | null;
    /** Inside the tickets list: tighter padding, the list owns Esc. */
    embedded?: boolean;
  }

  let { item, projectId, embedded = false }: Props = $props();

  const ticket = $derived(item.ticket);
  const ref = $derived(ticket.ref);
  const key = $derived(ticketKey(ref));
  const slot = $derived(tickets.details[key]);
  const detail = $derived(slot?.data ?? null);
  const project = $derived(projectId ?? item.project_ids[0] ?? projects.activeId);
  const workItem = $derived(work.forTicket(ref) ?? (item.work_item_id ? work.get(item.work_item_id) : null));
  const branch = $derived(workItem?.branch || mainPr(item.prs)?.branch || null);
  const proposal = $derived(refines[key] ?? null);

  let root = $state<HTMLDivElement>();
  let statusBtn = $state<HTMLElement>();
  let commentBox = $state<HTMLDivElement>();
  let picker = $state<HTMLElement | null>(null);
  let prMenu = $state<{ x: number; y: number; browser: boolean } | null>(null);
  /** The assignee (`u`) or priority (`!`) picker, under its meta row (TICKETS.md T8). */
  let field = $state<{ kind: 'assignee' | 'priority'; x: number; y: number } | null>(null);
  let comment = $state('');
  let posting = $state(false);

  // Keyed on the ticket key, not the `ref` object: list refreshes and patches make new objects.
  $effect(() => {
    void key;
    untrack(() => {
      // A reused instance (split view following the selection) must not carry a draft across.
      comment = '';
      picker = null;
      prMenu = null;
      field = null;
      loadCurrent();
      ensureReviews();
    });
  });

  // One `tracker_get` at a time: walking the split view with j/k loads where the selection lands,
  // not every ticket passed on the way.
  let inflight = false;
  function loadCurrent(): void {
    const s = tickets.details[key];
    // The standalone pane has just loaded it: no second `tracker_get` on mount.
    if (inflight || s?.loading || Date.now() - (s?.fetchedAt ?? 0) <= 2000) return;
    inflight = true;
    const loading = key;
    void tickets.loadDetail(ref).finally(() => {
      inflight = false;
      if (key !== loading) loadCurrent();
    });
  }

  async function assign(assignee: Assignee, done: string): Promise<void> {
    try {
      tickets.patch(await trackerAssign({ ticket: ref, assignee }));
      toasts.info(done);
    } catch (err) {
      toasts.error(err, `Assigning ${ref.key}`);
    }
  }

  async function setPriority(priority: string): Promise<void> {
    try {
      tickets.patch(await trackerSetPriority({ ticket: ref, priority }));
      toasts.info(`${ref.key} priority set to ${priority}`);
    } catch (err) {
      toasts.error(err, `Changing the priority of ${ref.key}`);
    }
  }

  function openField(kind: 'assignee' | 'priority'): void {
    const reason = kind === 'assignee' ? blockedReason('assign', item, branch) : null;
    if (reason) return void toasts.info(reason);
    const r = root?.querySelector(`[data-field="${kind}"]`)?.getBoundingClientRect();
    priorities = null;
    field = { kind, x: r?.left ?? 40, y: (r?.bottom ?? 40) + 2 };
  }

  /** Names of the users the picker shows, for the toast. */
  let names: Record<string, string> = {};
  async function loadUsers(query: string): Promise<MenuItem[]> {
    const users = await trackerAssignableUsers({ ticket: ref, query });
    names = Object.fromEntries(users.map((u) => [u.id, u.name]));
    return users.map((u) => ({
      id: u.id,
      label: u.name,
      detail: [u.login && u.login !== u.name ? u.login : '', u.id === ticket.assignee?.id ? 'assigned' : '']
        .filter(Boolean)
        .join(', '),
    }));
  }

  // One `tracker_priorities` per open picker: the filter narrows the list it returned.
  let priorities: Promise<string[]> | null = null;
  async function loadPriorities(query: string): Promise<MenuItem[]> {
    priorities ??= trackerPriorities({ ticket: ref }).catch((err: unknown) => {
      priorities = null;
      throw err;
    });
    const q = query.toLowerCase();
    return (await priorities)
      .filter((p) => p.toLowerCase().includes(q))
      .map((p) => ({ id: p, label: p, detail: p === ticket.priority ? 'current' : '' }));
  }

  function pickField(id: string): void {
    if (field?.kind === 'priority') void setPriority(id);
    else void assign({ kind: 'user', id }, `${ref.key} assigned to ${names[id] ?? id}`);
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

  async function postProposal(): Promise<void> {
    const text = proposal?.text;
    if (!text || posting) return;
    posting = true;
    try {
      await trackerComment({ ticket: ref, markdown: text });
      if (refines[key]) refines[key].posted = true;
      toasts.info(`Refinement posted on ${ref.key}`);
      void tickets.loadDetail(ref);
    } catch (err) {
      toasts.error(err, `Commenting on ${ref.key}`);
    } finally {
      posting = false;
    }
  }

  async function copyProposal(): Promise<void> {
    if (!proposal?.text) return;
    try {
      await clipboardWrite({ kind: 'clipboard', text: proposal.text });
      toasts.info('Copied the refinement');
    } catch (err) {
      toasts.error(err, 'Copy');
    }
  }

  async function copyBranch(): Promise<void> {
    if (!branch) return;
    try {
      await clipboardWrite({ kind: 'clipboard', text: branch });
      toasts.info(`Copied ${branch}`);
    } catch (err) {
      toasts.error(err, 'Copy branch');
    }
  }

  function openPrFrom(browser: boolean): void {
    // No project to open Kelta's review detail in: the browser.
    if (!openPrs(ref.key, item.prs, project ?? '', browser || !project)) return;
    const r = root?.querySelector('[data-action="pr"]')?.getBoundingClientRect();
    prMenu = { x: r?.left ?? 40, y: (r?.bottom ?? 40) + 2, browser };
  }

  /** Runs an action, or says why it cannot run (a key press on an off action). */
  function run(action: TicketAction, browser = false): void {
    const reason = blockedReason(action, item, branch);
    if (reason) return void toasts.info(reason);
    switch (action) {
      case 'start':
        void startWorkOnTicket(ref, project, browser ? { preview: false } : {});
        break;
      case 'move':
        picker = statusBtn ?? null;
        break;
      case 'pr':
        openPrFrom(browser);
        break;
      case 'assign':
        void assign({ kind: 'me' }, `${ref.key} assigned to you`);
        break;
      case 'unassign':
        void assign({ kind: 'none' }, `${ref.key} unassigned`);
        break;
      case 'comment':
        commentBox?.querySelector('textarea')?.focus();
        break;
      case 'refine':
        void refine(ref, project);
        break;
      case 'branch':
        void copyBranch();
        break;
      case 'browser':
        openExternal({ url: ticket.url }).catch((err) => toasts.error(err, 'Open in browser'));
        break;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    const own = 'input, textarea, select, [role="dialog"], [role="menu"], [data-child]';
    if ((e.target as HTMLElement).closest(own)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const action = TICKET_ACTIONS.find((a) => a.key === e.key);
    // Shift variants: `S` starts with no sheet, `P` opens the PR in the browser.
    if (action) run(action.id);
    else if (e.key === 'S') run('start', true);
    else if (e.key === 'P') run('pr', true);
    else if (e.key === 'R') void tickets.loadDetail(ref);
    else if (e.key === 'u') openField('assignee');
    else if (e.key === '!') openField('priority');
    else return;
    e.preventDefault();
  }

  function openTicket(ticket: TicketRef): void {
    if (project) void openContent(project, { kind: 'ticket_detail', ticket });
  }

  /** A focused sub-task row: `s` starts work on it (`S` with no sheet); its other keys do nothing. */
  function childKeys(e: KeyboardEvent, child: TicketItem): void {
    if (e.metaKey || e.ctrlKey || e.altKey || (e.key !== 's' && e.key !== 'S')) return;
    e.preventDefault();
    const opts = e.key === 'S' ? { preview: false } : {};
    void startWorkOnTicket(child.ticket.ref, child.project_ids[0] ?? project, opts);
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

  /** `YYYY-MM-DD` is a calendar day (no timezone shift); RFC 3339 a moment. */
  function day(s: string): Date {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
    return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : new Date(s);
  }

  const CI_WORDS: Record<CiState, string> = {
    success: 'CI passed',
    failure: 'CI failed',
    error: 'CI error',
    pending: 'CI running',
    none: '',
  };

  const dateFmt = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });
  /** Before today's midnight (read once per ticket: no clock ticking in the pane). */
  const overdue = $derived(
    ticket.due !== null &&
      ticket.status.category !== 'done' &&
      day(ticket.due).getTime() < day(new Date().toLocaleDateString('sv')).getTime(),
  );
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="detail"
  class:embedded
  data-testid="ticket-detail"
  bind:this={root}
  tabindex="0"
  role="group"
  aria-label={`Ticket ${ref.key}`}
  {onkeydown}
>
  <StateBanner
    stale={slot?.stale ?? false}
    fetchedAt={slot?.fetchedAt}
    error={detail ? slot?.error : null}
    onretry={() => void tickets.loadDetail(ref)}
  />
  <header class="head">
    <div class="line">
      <span class="key k-selectable">{ref.key}</span>
      {#if detail?.parent}
        {@const parent = detail.parent}
        <button type="button" class="link" onclick={() => openTicket(parent)}>
          parent {parent.key}
        </button>
      {/if}
      {#if workItem}<Badge tone="accent">local work</Badge>{/if}
    </div>
    <h1>{ticket.title}</h1>
    <div class="line">
      <span class="status" bind:this={statusBtn}>
        <Button
          aria-haspopup="menu"
          aria-label={`Status ${ticket.status.name}, move (m)`}
          title="Move (m)"
          onclick={() => (picker = statusBtn ?? null)}
        >
          <StatusChip status={ticket.status} />
          <Icon name="chevron-down" size={14} />
        </Button>
      </span>
    </div>
    <ActionBar {item} {branch} resume={workItem !== null} onrun={(a) => run(a)} />
  </header>

  <div class="scroll">
    <dl class="k-meta">
      <dt>Assignee</dt>
      <dd>
        <button
          type="button"
          class="field"
          data-field="assignee"
          aria-haspopup="menu"
          aria-keyshortcuts="u"
          title="Assign to… (u)"
          onclick={() => openField('assignee')}>{ticket.assignee?.name ?? 'Unassigned'}</button
        >
      </dd>
      <dt>Priority</dt>
      <dd>
        <button
          type="button"
          class="field"
          data-field="priority"
          aria-haspopup="menu"
          aria-keyshortcuts="Shift+1"
          title="Change priority (!)"
          onclick={() => openField('priority')}>{ticket.priority ?? 'None'}</button
        >
      </dd>
      <dt>Sprint</dt>
      <dd>
        {#if ticket.sprint}
          {ticket.sprint.name}
          {#if ticket.sprint.ends_at}<span class="muted"
              >ends {dateFmt.format(day(ticket.sprint.ends_at))}</span
            >{/if}
        {:else}<span class="muted">None</span>{/if}
      </dd>
      <dt>Estimate</dt>
      <dd>
        {#if ticket.estimate}{ticket.estimate}{:else}<span class="muted">None</span>{/if}
      </dd>
      <dt>Due</dt>
      <dd class="k-num">
        {#if ticket.due}
          <span class:late={overdue} title={overdue ? 'Overdue' : undefined}
            >{dateFmt.format(day(ticket.due))}</span
          >
          {#if overdue}<span class="late">overdue</span>{/if}
        {:else}<span class="muted">None</span>{/if}
      </dd>
      <dt>Updated</dt>
      <dd class="k-num">{relativeTime(Date.parse(ticket.updated_at))}</dd>
      {#if ticket.kind}<dt>Type</dt>
        <dd>{ticket.kind}</dd>{/if}
      {#if ticket.labels.length > 0}<dt>Labels</dt>
        <dd>
          {#each ticket.labels as l (l)}<Badge>{l}</Badge>{/each}
        </dd>{/if}
    </dl>

    <section aria-label="Pull requests" data-testid="ticket-pr">
      <h2>
        Pull requests {#if item.prs.length > 1}<span class="muted k-num">{item.prs.length}</span>{/if}
      </h2>
      {#each item.prs as pr (pr.url)}
        <div class="pr">
          <button
            type="button"
            class="link k-mono"
            title={pr.account ? 'Open in Kelta' : 'Open in browser'}
            onclick={() => openPr(pr, project ?? '', !project)}>{prLabel(pr)}</button
          >
          <span class="pr-title">{pr.title || pr.repo}</span>
          <span class="pr-meta">
            <span>{reviewWord(pr)}</span>
            {#if pr.ci !== 'none'}<span class="ci"
                ><Lamp level={ciLamp(pr.ci)} title={`CI ${pr.ci}`} />{CI_WORDS[pr.ci]}</span
              >{/if}
            {#if pr.branch}<code class="k-mono">{pr.branch}</code>{/if}
          </span>
        </div>
      {:else}
        <p class="muted">No pull request yet.</p>
      {/each}
    </section>

    {#if detail && detail.children.length > 0}
      <section aria-label="Sub-tasks">
        <h2>Sub-tasks <span class="muted k-num">{detail.children.length}</span></h2>
        {#each detail.children as c (c.ticket.ref.key)}
          <button
            type="button"
            class="child"
            data-child
            aria-keyshortcuts="s"
            title="Open (Enter), start work (s)"
            onclick={() => openTicket(c.ticket.ref)}
            onkeydown={(e) => childKeys(e, c)}
          >
            <span class="key">{c.ticket.ref.key}</span>
            <StatusChip status={c.ticket.status} />
            <span class="pr-title">{c.ticket.title}</span>
          </button>
        {/each}
      </section>
    {/if}

    {#if proposal}
      <section aria-label="Refinement" data-testid="ticket-refine">
        <h2>
          Refinement {#if proposal.running}<span class="muted">Claude is working…</span>{/if}
        </h2>
        {#if proposal.text !== null}
          <pre class="proposal">{proposal.text}</pre>
          <div class="refine-actions">
            <Button
              size="sm"
              variant="primary"
              disabled={proposal.posted || !item.caps.comment}
              loading={posting}
              title={item.caps.comment ? undefined : 'This tracker does not let Kelta add comments.'}
              onclick={() => void postProposal()}>{proposal.posted ? 'Posted' : 'Post as comment'}</Button
            >
            <Button size="sm" onclick={() => void copyProposal()}>Copy</Button>
            <Button size="sm" variant="ghost" onclick={() => discard(ref)}>Discard</Button>
          </div>
          <p class="muted">Start work adds these acceptance criteria to Claude's first prompt.</p>
        {/if}
      </section>
    {/if}

    <section aria-label="Description">
      <h2>Description</h2>
      {#if !detail}
        <p class="muted">
          {slot?.error ? `Could not load the description: ${slot.error.message}` : 'Loading…'}
        </p>
      {:else if detail.body_html.trim() === ''}
        <p class="muted">No description.</p>
      {:else}
        <HtmlContent html={detail.body_html} onerror={(e) => toasts.error(e, 'Open link')} class="body" />
      {/if}
    </section>

    <section aria-label="Comments">
      <h2>
        Comments {#if detail}<span class="muted k-num">{detail.comments.length}</span>{/if}
      </h2>
      {#each (detail?.comments ?? []).slice(-20) as c, i (i)}
        <article class="comment">
          <header>
            <strong>{c.author.name}</strong>
            <span class="muted">{relativeTime(Date.parse(c.created_at))}</span>
          </header>
          <HtmlContent html={c.body_html} onerror={(e) => toasts.error(e, 'Open link')} />
        </article>
      {:else}
        {#if detail}<p class="muted">No comments yet.</p>{/if}
      {/each}
      {#if item.caps.comment}
        <div bind:this={commentBox} class="compose">
          <TextInput
            bind:value={comment}
            label="Add a comment (Markdown, Ctrl/Cmd+Enter to send)"
            multiline
            rows={embedded ? 3 : 4}
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
      {/if}
    </section>
  </div>
</div>

{#if picker}
  <StatusPicker
    tickets={[ticket]}
    {projectId}
    anchor={picker}
    onclose={() => {
      picker = null;
      root?.focus();
    }}
  />
{/if}
{#if field}
  <PickMenu
    label={field.kind === 'assignee' ? `Assign ${ref.key}` : `Priority of ${ref.key}`}
    placeholder={field.kind === 'assignee' ? 'Find a person' : 'Filter priorities'}
    x={field.x}
    y={field.y}
    load={field.kind === 'assignee' ? loadUsers : loadPriorities}
    onselect={pickField}
    onclose={() => {
      field = null;
      root?.focus();
    }}
  />
{/if}
{#if prMenu}
  {@const browser = prMenu.browser}
  <Menu
    items={prMenuItems(item.prs)}
    x={prMenu.x}
    y={prMenu.y}
    label={`Pull requests of ${ref.key}`}
    onselect={(url) => {
      const pr = item.prs.find((p) => p.url === url);
      if (pr) openPr(pr, project ?? '', browser || !project);
    }}
    onclose={() => {
      prMenu = null;
      root?.focus();
    }}
  />
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    container-type: inline-size;
    outline: none;
    background: var(--k-well);
    color: var(--k-fg);
  }

  /* Split view: list and detail share one pane, so mark which side has the keys (DESIGN §7). */
  .detail.embedded:focus-visible {
    box-shadow: inset 2px 0 0 var(--k-focus);
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    padding: var(--k-space-5) var(--k-space-5) var(--k-space-4);
  }

  .embedded .head {
    padding: var(--k-space-4) var(--k-space-4) var(--k-space-3);
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

  .late {
    color: var(--k-danger);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--k-accent);
    font: inherit;
    cursor: pointer;
  }

  .status {
    display: inline-flex;
  }

  /* A meta value that opens its picker: plain text until hovered. */
  .field {
    padding: 0;
    border: 0;
    border-bottom: 1px dashed transparent;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .field:hover,
  .field:focus-visible {
    border-bottom-color: var(--k-fg-subtle);
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

  .embedded .scroll {
    padding: 0 var(--k-space-4) var(--k-space-4);
  }

  /* One PR per line: label, title filling, then state words; wraps under the title when narrow. */
  .pr {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--k-space-1) var(--k-space-4);
    max-width: var(--k-measure);
    padding: var(--k-space-1) 0;
  }

  .pr-title {
    flex: 1;
    min-width: 12ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pr-meta {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-1) var(--k-space-4);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
  }

  .ci {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .pr-meta code {
    font-size: var(--k-font-size-xs);
  }

  .child {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 100%;
    max-width: var(--k-measure);
    padding: var(--k-space-1) var(--k-space-2);
    border: 0;
    border-radius: var(--k-radius-sm);
    background: transparent;
    color: var(--k-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .child:hover,
  .child:focus-visible {
    background: var(--k-bezel-raised);
  }

  /* Task lists are the tracker's to tick: shown, never toggled here. */
  section :global(.k-html input[type='checkbox']) {
    margin: 0 var(--k-space-2) 0 0;
    vertical-align: -1px;
    pointer-events: none;
  }

  section :global(.k-html li:has(> input[type='checkbox'])) {
    list-style: none;
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

  .proposal {
    max-width: var(--k-measure);
    margin: 0;
    padding: var(--k-space-3);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--k-bezel-raised);
    border-radius: var(--k-radius-sm);
  }

  .refine-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2);
    margin-top: var(--k-space-3);
  }
</style>
