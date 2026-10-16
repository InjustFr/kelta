<script lang="ts">
  import { t, tn } from '$lib/i18n/index.svelte';
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import type { ProjectId, ProjectInfo, SessionInfo, Tab, WorkItem } from '$lib/gen';
  import { allPanes } from '$lib/layout';
  import {
    layout,
    projects,
    reviews,
    sessions,
    tickets,
    toasts,
    tools,
    ui,
    work,
    type GlobalView,
  } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import { Button, Icon, IconButton, Kbd, Lamp, Menu, type MenuItem } from '$lib/ui';
  import { currentPlatform, formatChord } from '$lib/ui/format';
  import { frames } from '$lib/terminal/raf';

  import { jumpOutside, nowSummary, refreshNow } from '../views/inbox/now';
  import { canMakeWorkItem, phaseNow, prOf, siblingKey, stepNow, workTitle } from '../views/work/live';
  import { maxLamp, outsidePhase, workKey } from '../views/work/phase';
  import { stageName, workStage } from '../views/work/stage';
  import { confirms } from './confirm.svelte';
  import { flushJump } from './jumplist';
  import { chordFor, runLine, sessionTitle, statusLabel } from './labels';
  import {
    activateProject,
    attachTab,
    backgroundRows,
    bandTabs,
    detachTab,
    openRuns,
    liveSessionsOfTab,
    openRow,
    openView,
    railProjects,
    railToolsOf,
    requestCloseTab,
    requestStopSession,
    rowLabel,
    rowLamp,
    tabAttention,
    VIEW_NAMES,
    workRows,
    type RailRow,
  } from './nav';
  import { droppedOutside, paneDrag } from './paneDrag.svelte';
  import { peekHover, peekTarget } from './peek.svelte';
  import { SIDEBAR_MAX, SIDEBAR_MIN, sidebar } from './sidebar.svelte';

  const rail = $derived(railProjects());
  const reorderable = $derived(rail.filter((p) => !p.builtin));

  // One count per view, on its sub line (#430); no red badge: the lit rows say what waits.
  const now = $derived(nowSummary());
  const ALL = { kind: 'all' } as const;
  const mine = $derived(
    tickets.items(ALL, null, 'mine').filter((i) => i.ticket.status.category !== 'done').length,
  );
  const toReview = $derived(reviews.list(ALL, 'review_requested').data?.items.length ?? 0);

  const toolsOf = $derived(railToolsOf());
  // `project.goto.N` index of each work row.
  const index = $derived(new Map(workRows().map((r, i) => [r.item!.id, i + 1])));
  // Home has no band (#435): its rows go under Other Claude sessions, its shells fold at the foot.
  const home = $derived(rail.find((p) => p.builtin) ?? null);
  const homeBand = $derived(home ? bandTabs(home.id) : null);
  const homeRows = $derived(homeBand ? [...homeBand.work, ...homeBand.claude] : []);
  const homeFolded = $derived(homeBand?.more.length ?? 0);
  // Live sessions with no tab, every project's (#452): one section above the footer.
  const background = $derived(backgroundRows());
  // Bands whose "n more" row is open.
  let unfolded = $state<Record<ProjectId, boolean>>({});
  // The tool list behind the tool keys and the palette.
  $effect(() => {
    if (tools.byProject[toolsOf] === undefined) void tools.load(toolsOf);
  });

  // Background runs' claims on tickets (#392): a claimed run shows on its ticket, not on its session's row.
  const claims = $derived(work.all.flatMap((w) => (w.run ? [w.run] : [])));
  // Runs a Claude session started before this window listened (once per session; events keep them current).
  $effect(() => {
    for (const s of sessions.all)
      if (s.kind.type === 'claude' && s.lifecycle === 'live') void sessions.loadRuns(s.id);
  });

  // Startup and window focus refresh Now's sources (no polling).
  onMount(() => void refreshNow());

  let menu = $state<{ project: ProjectInfo; x: number; y: number } | null>(null);
  let rowMenu = $state<{
    project: ProjectId;
    tab: Tab;
    session: SessionInfo | null;
    x: number;
    y: number;
  } | null>(null);
  let dragId = $state<string | null>(null);
  let overId = $state<string | null>(null);
  // A row dragged out of the window moves to its own window; rows keep their order (#430).
  let rowDrag = $state<{ project: ProjectId; tab: string } | null>(null);
  let rowOver = $state<string | null>(null);
  // A pane dragged over a band but not over one of its rows: dropping opens it in a new tab.
  let bandOver = $state<ProjectId | null>(null);

  function menuItems(p: ProjectInfo): MenuItem[] {
    const index = reorderable.findIndex((x) => x.id === p.id);
    const items: MenuItem[] = [
      { id: 'settings', label: t('Project settings…'), icon: 'settings' },
      { id: 'add', label: t('Add project'), icon: 'plus' },
    ];
    if (!p.builtin) {
      items.push(
        { id: 'up', label: t('Move up'), icon: 'arrow-up', disabled: index <= 0, separator: true },
        { id: 'down', label: t('Move down'), icon: 'arrow-down', disabled: index >= reorderable.length - 1 },
        { id: 'close', label: t('Close project'), icon: 'x', separator: true },
        { id: 'close_kill', label: t('Close and stop sessions'), icon: 'square', danger: true },
        { id: 'remove', label: t('Remove project…'), icon: 'trash', danger: true },
      );
    }
    return items;
  }

  async function onMenu(p: ProjectInfo, id: string): Promise<void> {
    try {
      switch (id) {
        case 'settings':
          await dispatch('settings.open', { section: 'projects', project_id: p.id });
          break;
        case 'add':
          await dispatch('project.new');
          break;
        case 'up':
        case 'down':
          await reorder(p.id, indexOf(p.id) + (id === 'up' ? -1 : 1));
          break;
        case 'close':
          await closeProject(p, false);
          break;
        case 'close_kill': {
          const n = sessions.forProject(p.id).filter((s) => s.lifecycle === 'live').length;
          const answer = await confirms.ask({
            title: t('Close {name} and stop its sessions?', { name: p.name }),
            body: tn(
              '{n} running session will be terminated.',
              '{n} running sessions will be terminated.',
              n,
            ),
            tone: 'danger',
            actions: [{ id: 'stop', label: t('Close and stop sessions'), variant: 'danger' }],
          });
          if (answer === 'stop') await closeProject(p, true);
          break;
        }
        case 'remove': {
          const answer = await confirms.ask({
            title: t('Remove {name}?', { name: p.name }),
            body: t('The project configuration is moved to projects/.trash. Running sessions keep running.'),
            tone: 'danger',
            actions: [{ id: 'remove', label: t('Remove project'), variant: 'danger' }],
          });
          if (answer === 'remove') {
            const wasActive = p.active;
            await projects.remove(p.id, false);
            if (wasActive) fallbackActive();
          }
          break;
        }
      }
    } catch (err) {
      toasts.error(err, t('Project action failed'));
    }
  }

  function fallbackActive(): void {
    const next = railProjects()[0];
    if (next) void activateProject(next.id);
  }

  async function closeProject(p: ProjectInfo, kill: boolean): Promise<void> {
    const wasActive = p.active;
    await projects.close(p.id, kill);
    if (wasActive) fallbackActive();
  }

  /** Moves `fromId` to `toIndex` among the reorderable projects; out of range does nothing. */
  async function reorder(fromId: string, toIndex: number): Promise<void> {
    const ids = reorderable.map((p) => p.id);
    const at = ids.indexOf(fromId);
    if (at < 0 || toIndex < 0 || toIndex >= ids.length) return;
    ids.splice(toIndex, 0, ...ids.splice(at, 1));
    await applyOrder(ids);
  }

  const indexOf = (id: string) => reorderable.findIndex((p) => p.id === id);

  /** Writes the new order of the reorderable projects back into the full list order. */
  async function applyOrder(orderedOpen: string[]): Promise<void> {
    const openSet = new Set(reorderable.map((p) => p.id));
    const queue = [...orderedOpen];
    const all = projects.list.map((p) => (openSet.has(p.id) ? (queue.shift() ?? p.id) : p.id));
    await projects.reorder(all);
  }

  async function onDrop(targetId: string): Promise<void> {
    const from = dragId;
    dragId = null;
    overId = null;
    if (!from || from === targetId) return;
    try {
      await reorder(from, indexOf(targetId));
    } catch (err) {
      toasts.error(err, t('Reordering projects failed'));
    }
  }

  function openMenu(e: MouseEvent, p: ProjectInfo): void {
    e.preventDefault();
    menu = { project: p, x: e.clientX, y: e.clientY };
  }

  function rowMenuItems(tab: Tab, session: SessionInfo | null): MenuItem[] {
    return [
      { id: 'close', label: t('Close'), icon: 'x' },
      tab.detached
        ? { id: 'attach', label: t('Move back to main window'), icon: 'app-window' }
        : { id: 'detach', label: t('Move to new window'), icon: 'app-window' },
      ...(session && canMakeWorkItem(session)
        ? [{ id: 'make_work', label: t('Make this a work item'), icon: 'git-branch' }]
        : []),
    ];
  }

  function onRowMenu(project: ProjectId, tab: Tab, id: string, session: SessionInfo | null): void {
    if (id === 'make_work' && session) void dispatch('session.make_work', { session_id: session.id });
    else if (id === 'close') void requestCloseTab(project, tab.id);
    else if (id === 'detach') void detachTab(project, tab.id);
    else if (id === 'attach') void attachTab(project, tab.id);
  }

  /** A work row's title, however its tab was opened (#435): the ticket's or PR's, else its tab's after the key. */
  function itemTitle(item: WorkItem, tab: Tab | null, key: string): string {
    const title = workTitle(item);
    if (title !== item.branch || !tab?.title.startsWith(key)) return title;
    return tab.title.slice(key.length).trim() || title;
  }

  /** The stage word on the right of a work row: its pipeline step (#462), else Claude, PR, Review, Merge or Deploy. */
  function stageWord(item: WorkItem): string {
    return stageName(workStage(item, phaseNow(item), prOf(item), false, false, stepNow(item)));
  }

  function rowIcon(tab: Tab): string {
    if (tab.detached) return 'app-window';
    if (tab.work_item_id) return 'git-branch';
    if (liveSessionsOfTab(tab).some((s) => s.kind.type === 'claude')) return 'bot';
    return allPanes(tab.root).some((p) => p.content.kind === 'terminal') ? 'terminal' : 'app-window';
  }

  function isFront(project: ProjectId, tabId: string): boolean {
    return (
      !ui.view && !ui.settings && projects.activeId === project && layout.get(project)?.active_tab === tabId
    );
  }

  // The `⌘1`–`⌘9` digits show only while the modifier is held (#451); the row tooltips always name the chord.
  let modHeld = $state(false);
  const onModKey = (e: KeyboardEvent) => (modHeld = currentPlatform() === 'macos' ? e.metaKey : e.ctrlKey);

  /** `label (Ctrl+K)`: the tooltip keeps the chord when its badge has no room (#362). */
  function tip(label: string, id: string): string {
    const c = chordFor(id);
    return c ? `${label} (${formatChord(c).join(currentPlatform() === 'macos' ? '' : '+')})` : label;
  }

  // ---- width: drag or arrow keys on the right edge (#362) ----

  let nav = $state<HTMLElement>();
  let dragging = $state(false);
  let dragX = 0;
  const dragKey = {};

  function onEdgeDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragX = sidebar.width; // a click without movement must not reuse a stale drag position
    dragging = true;
  }

  function onEdgeMove(e: PointerEvent): void {
    if (!dragging || !nav) return;
    dragX = e.clientX - nav.getBoundingClientRect().left;
    // One resize per frame: the terminals refit on every width change.
    frames.schedule(dragKey, () => sidebar.resize(dragX, false));
  }

  function onEdgeUp(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    frames.cancel(dragKey);
    sidebar.resize(dragX);
  }

  function onEdgeKey(e: KeyboardEvent): void {
    const step = e.shiftKey ? 64 : 16;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      sidebar.resize(sidebar.width + (e.key === 'ArrowLeft' ? -step : step));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      sidebar.toggle();
    }
  }
</script>

<svelte:window
  onfocus={() => void refreshNow()}
  onblur={() => (modHeld = false)}
  onkeydowncapture={onModKey}
  onkeyupcapture={onModKey}
/>

{#snippet chord(id: string)}
  {@const c = chordFor(id)}
  {#if c}<span class="kbd" aria-hidden="true"><Kbd chord={c} /></span>{/if}
{/snippet}

<!-- A global view (SPEC §1): shown outside any project; the active project stays as it is. -->
{#snippet place(view: GlobalView, icon: string, id: string, sub: string, n: number, chordId?: string)}
  {@const name = VIEW_NAMES[view]}
  <button
    type="button"
    class="item place"
    class:active={ui.view === view && !ui.settings}
    onclick={() => {
      flushJump(); // the place this click leaves is a stop on the way back, settled or not
      openView(view);
    }}
    title={`${name}: ${sub}`}
    aria-current={ui.view === view && !ui.settings ? 'page' : undefined}
    data-testid={`rail-${id}`}
  >
    <span class="tile plain"><Icon name={icon} size={18} /></span>
    <span class="text">
      <span class="name">{name}</span>
      <span class="sub">{sub}</span>
    </span>
    {#if chordId}{@render chord(chordId)}{/if}
    {#if n > 0}
      <span class="badge k-num" data-testid={`${id}-badge`}>{n > 99 ? '99+' : n}</span>
    {/if}
  </button>
{/snippet}

<!-- A row's workflow run (#392): opens the session's runs beside its terminal. -->
{#snippet runRow(run: { text: string; sessionId: string }, label: string)}
  <li class="row">
    <button
      type="button"
      class="item instance"
      title={`${label} › ${t('Workflow runs of this session')}`}
      data-testid="rail-run"
      data-session-id={run.sessionId}
      onclick={() => void openRuns(run.sessionId)}
    >
      <span class="text"><span class="name">└ {run.text}</span></span>
    </button>
  </li>
{/snippet}

<!-- The muted `⌘N` index of a row, when that chord is bound and its modifier held. -->
{#snippet digit(id: string, text: string)}
  {@const c = chordFor(id)}
  {#if c && modHeld}<span class="index k-num" title={formatChord(c).join('')} aria-hidden="true">{text}</span
    >{/if}
{/snippet}

{#snippet more(p: ProjectInfo)}
  <IconButton
    class="more"
    size="sm"
    icon="ellipsis"
    label={t('Menu for {name}', { name: p.name })}
    onclick={(e) => openMenu(e, p)}
  />
{/snippet}

<!-- A band row: a work item (lamp, key, title, stage word), a Claude session (lamp, ✻, name, status
     word) or, unfolded, any other tab. Without a tab it is a Background row (#452): dimmed, a click
     reveals its session in a tab, `×` stops it. -->
{#snippet row(p: ProjectInfo, r: RailRow)}
  {@const tab = r.tab}
  {@const item = r.item ?? null}
  {@const s = r.session ?? null}
  {@const lvl = rowLamp(r)}
  {@const active = !!tab && isFront(p.id, tab.id)}
  {@const key = item ? workKey(item) : ''}
  {@const label = item
    ? `${siblingKey(item)} ${itemTitle(item, tab, key)}`
    : s
      ? sessionTitle(s)
      : rowLabel(tab!)}
  {@const n = item && tab ? index.get(item.id) : undefined}
  {@const live = s ? [s] : tab ? liveSessionsOfTab(tab) : []}
  {@const run = runLine(
    item?.run,
    live.flatMap((x) => sessions.runs[x.id] ?? []),
    claims,
  )}
  <li
    class="row"
    class:pane-over={!!tab && rowOver === tab.id && paneDrag.current && paneDrag.current.tabId !== tab.id}
    style:--project-color={tab ? undefined : (resolveProjectColor(p.color) ?? 'var(--k-border-strong)')}
  >
    <button
      type="button"
      class="item tab-row"
      class:active
      class:background={!tab}
      class:lit={lvl === 'needs_input'}
      title={n ? tip(`${p.name} › ${label}`, `project.goto.${n}`) : `${p.name} › ${label}`}
      aria-current={active ? 'page' : undefined}
      data-testid={tab ? 'rail-row' : 'rail-background'}
      data-project-id={p.id}
      data-tab-id={tab?.id}
      data-work-id={item?.id}
      data-session-id={s?.id}
      data-attention={lvl}
      use:peekHover={() => peekTarget(live)}
      draggable={tab ? 'true' : undefined}
      onclick={() => openRow(r)}
      onauxclick={(e) => {
        if (e.button !== 1 || !tab) return;
        e.preventDefault();
        void requestCloseTab(p.id, tab.id);
      }}
      onmousedown={(e) => {
        if (e.button === 1) e.preventDefault(); // no autoscroll on middle click
      }}
      oncontextmenu={(e) => {
        e.preventDefault();
        if (tab) rowMenu = { project: p.id, tab, session: s ?? null, x: e.clientX, y: e.clientY };
      }}
      ondragstart={(e) => {
        if (!tab) return;
        rowDrag = { project: p.id, tab: tab.id };
        e.dataTransfer?.setData('text/plain', tab.id);
      }}
      ondragover={(e) => {
        if (!tab || paneDrag.current?.projectId !== p.id) return;
        e.preventDefault();
        rowOver = tab.id;
      }}
      ondragleave={() => (rowOver = null)}
      ondrop={(e) => {
        if (!tab) return;
        e.preventDefault();
        rowOver = null;
        if (paneDrag.current) paneDrag.drop(p.id, { tabId: tab.id });
      }}
      ondragend={(e) => {
        if (tab && rowDrag && droppedOutside(e)) void detachTab(p.id, tab.id);
        rowDrag = null;
        rowOver = null;
      }}
    >
      <span class="socket" aria-hidden="true"><Lamp level={lvl} /></span>
      {#if item}
        {#if tab?.detached}<span class="kind" aria-hidden="true"><Icon name="app-window" size={14} /></span
          >{/if}
        <span class="key k-num">{key}</span>
        <span class="text"><span class="name">{itemTitle(item, tab, key)}</span></span>
        <!-- A work item's other background sessions (nvim, a shell) name themselves, so its rows differ. -->
        <span class="stage" data-testid="rail-stage"
          >{s && !tab && s.kind.type !== 'claude' ? sessionTitle(s) : stageWord(item)}</span
        >
      {:else if s}
        {#if s.kind.type === 'claude'}<span class="kind glyph" aria-hidden="true">✻</span>{:else}<span
            class="kind"
            aria-hidden="true"><Icon name="terminal" size={14} /></span
          >{/if}
        <span class="text"><span class="name">{sessionTitle(s)}</span></span>
        <span class="stage" data-testid="rail-stage">{statusLabel(s.status)}</span>
      {:else if tab}
        <span class="kind" aria-hidden="true"><Icon name={rowIcon(tab)} size={14} /></span>
        <span class="text"><span class="name">{label}</span></span>
      {/if}
      {#if n}{@render digit(`project.goto.${n}`, `${n}`)}{/if}
    </button>
    {#if tab}
      <IconButton
        class="close"
        size="sm"
        icon="x"
        label={t('Close {name}', { name: label })}
        onclick={() => void requestCloseTab(p.id, tab.id)}
      />
    {:else if s}
      <IconButton
        class="close"
        size="sm"
        icon="x"
        label={t('Close {name}', { name: label })}
        onclick={() => void requestStopSession(s)}
      />
    {/if}
  </li>
  {#if run}{@render runRow(run, label)}{/if}
{/snippet}

<!-- A band's folded row: its other tabs, opened in place; hidden when compact (#435). -->
{#snippet fold(p: ProjectInfo, more: Tab[], name: (n: number) => string)}
  {@const folded = more.length}
  {@const open = !!unfolded[p.id]}
  {#if folded > 0}
    {@const lvl = maxLamp(more.map((x) => tabAttention(x)))}
    <li class="row fold-row">
      <button
        type="button"
        class="item tab-row fold sess"
        class:active={!open && more.some((x) => isFront(p.id, x.id))}
        title={`${p.name} › ${name(folded)}`}
        aria-expanded={open}
        data-testid="rail-more"
        data-project-id={p.id}
        data-attention={lvl}
        onclick={() => (unfolded[p.id] = !open)}
      >
        <span class="socket" aria-hidden="true"><Lamp level={lvl} /></span>
        <span class="kind" aria-hidden="true"><Icon name="terminal" size={13} /></span>
        <span class="text"><span class="name">{name(folded)}</span></span>
      </button>
    </li>
    {#if open && !sidebar.compact}
      {#each more as tab (tab.id)}{@render row(p, { project: p.id, tab })}{/each}
    {/if}
  {/if}
{/snippet}

<nav
  class="rail"
  class:compact={sidebar.compact}
  aria-label={t('Projects')}
  data-testid="rail"
  style:--k-rail-width="{sidebar.width}px"
  bind:this={nav}
>
  {@render place('now', 'inbox', 'inbox', t('{n} your move', { n: now.yours }), now.yours, 'inbox.open')}
  {@render place('tickets', 'ticket', 'tickets', t('{n} assigned to you', { n: mine }), mine)}
  {@render place(
    'reviews',
    'git-pull-request',
    'reviews',
    tn('{n} asks for your review', '{n} ask for your review', toReview),
    toReview,
  )}

  <div class="group">
    <div class="scroll">
      {#each reorderable as p (p.id)}
        {@const band = bandTabs(p.id)}
        <section
          class="band"
          class:pane-target={bandOver === p.id && paneDrag.current}
          aria-label={p.name}
          data-testid="rail-band"
          data-project-id={p.id}
          style:--project-color={resolveProjectColor(p.color) ?? 'var(--k-border-strong)'}
          ondragover={(e) => {
            if (paneDrag.current?.projectId !== p.id) return;
            bandOver = e.defaultPrevented ? null : p.id; // a row took it
            e.preventDefault();
          }}
          ondragleave={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget as Node | null)) bandOver = null;
          }}
          ondrop={(e) => {
            bandOver = null;
            if (paneDrag.current?.projectId !== p.id) return;
            e.preventDefault();
            paneDrag.drop(p.id, { tabId: null });
          }}
        >
          <!-- The caption: click = the project's last row, ⋯ / right-click = project menu, drag = reorder. -->
          <div class="row caption-row" class:over={overId === p.id && dragId !== p.id}>
            <button
              type="button"
              class="caption"
              title={p.name}
              data-testid="rail-project"
              data-project-id={p.id}
              draggable="true"
              onclick={() => activateProject(p.id)}
              oncontextmenu={(e) => openMenu(e, p)}
              ondragstart={(e) => {
                dragId = p.id;
                e.dataTransfer?.setData('text/plain', p.id);
                if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
              }}
              ondragover={(e) => {
                if (!dragId) return;
                e.preventDefault();
                overId = p.id;
              }}
              ondragleave={() => (overId = null)}
              ondrop={(e) => {
                e.preventDefault();
                void onDrop(p.id);
              }}
              ondragend={() => {
                dragId = null;
                overId = null;
              }}
              ><span class="mark" aria-hidden="true"></span><span class="name">{p.name}</span><span
                class="count k-num"
                data-testid="rail-count">{band.work.length + band.claude.length}</span
              ></button
            >
            {@render more(p)}
          </div>
          <ul class="rows">
            {#each band.work as r (r.item!.id)}{@render row(p, r)}{/each}
            {#each band.claude as r (r.session!.id)}{@render row(p, r)}{/each}
            {@render fold(p, band.more, (n) => t('{n} more', { n }))}
          </ul>
        </section>
      {:else}
        <p class="none">{t('No projects yet')}</p>
      {/each}

      {#if projects.list.some((p) => !p.open)}
        <!-- Closed projects are not on the rail: this is the way back to them (#363). -->
        <div class="heading">
          <Button
            variant="ghost"
            size="sm"
            icon="folder-open"
            title={t('Reopen a closed project')}
            onclick={() => void dispatch('project.open')}
            data-testid="rail-reopen">{t('Open project…')}</Button
          >
        </div>
      {/if}

      {#if homeRows.length + sessions.elsewhere.length + homeFolded > 0}
        <!-- Claude sessions Kelta did not start (#345, dimmed, Now lists them) and Home's (#435); Home's shells at the foot. -->
        <section
          class="band tools"
          aria-label={t('Other Claude sessions')}
          data-testid="rail-elsewhere"
          style:--project-color={resolveProjectColor(home?.color) ?? 'var(--k-border-strong)'}
        >
          {#if homeRows.length + sessions.elsewhere.length > 0}
            <h2 class="tools-heading">{t('Other Claude sessions')}</h2>
          {/if}
          <ul class="rows">
            {#each homeRows as r (r.item?.id ?? r.session!.id)}{@render row(home!, r)}{/each}
            {#each sessions.elsewhere as s (s.id)}
              {@const ph = outsidePhase(s)}
              <li class="row">
                <button
                  type="button"
                  class="item tab-row background"
                  title={t('{title}: {label}', { title: s.title, label: ph.label }) +
                    (s.cwd ? `\n${s.cwd}` : '')}
                  data-testid="rail-outside"
                  data-attention={ph.lamp}
                  onclick={() => void jumpOutside(s.id)}
                >
                  <span class="socket" aria-hidden="true"><Lamp level={ph.lamp} /></span>
                  <span class="kind" aria-hidden="true"><Icon name="bot" size={14} /></span>
                  <span class="text"><span class="name">{s.title}</span></span>
                </button>
              </li>
            {/each}
            {#if home && homeBand}
              {@render fold(home, homeBand.more, (n) => tn('{n} shell', '{n} shells', n))}
            {/if}
          </ul>
        </section>
      {/if}
    </div>
  </div>

  {#if background.length > 0}
    <!-- Live sessions with no tab (#452), every project's in rail order; compact keeps the rows, not the caption. -->
    <section class="background-section" aria-label={t('Background')} data-testid="rail-background-section">
      <h2 class="tools-heading quiet">
        {t('Background')} <span class="count k-num">{background.length}</span>
      </h2>
      <ul class="rows">
        {#each background as r (r.session!.id)}{@render row(projects.byId(r.project)!, r)}{/each}
      </ul>
    </section>
  {/if}

  <div class="footer">
    <button
      type="button"
      class="item settings"
      class:active={ui.settings !== null}
      aria-current={ui.settings ? 'page' : undefined}
      title={tip(t('Settings'), 'settings.open')}
      onclick={() => void dispatch('settings.open')}
      data-testid="rail-settings"
    >
      <span class="lead">
        <span class="tile plain"><Icon name="settings" size={16} /></span>
        <span class="text"><span class="name">{t('Settings')}</span></span>
      </span>
      {@render chord('settings.open')}
    </button>
    <IconButton
      icon="panel-left"
      size="sm"
      label={sidebar.compact ? t('Expand sidebar') : t('Collapse sidebar')}
      chord={chordFor('rail.toggle')}
      aria-expanded={!sidebar.compact}
      onclick={() => sidebar.toggle()}
      data-testid="rail-toggle"
    />
  </div>

  {#if !sidebar.compact}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="edge"
      class:dragging
      role="separator"
      aria-label={t('Sidebar width')}
      aria-orientation="vertical"
      aria-valuenow={sidebar.width}
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      tabindex="0"
      data-testid="rail-edge"
      onpointerdown={onEdgeDown}
      onpointermove={onEdgeMove}
      onpointerup={onEdgeUp}
      onpointercancel={onEdgeUp}
      onkeydown={onEdgeKey}
    ></div>
  {/if}
</nav>

{#if rowMenu}
  {@const target = rowMenu}
  <Menu
    items={rowMenuItems(target.tab, target.session)}
    x={rowMenu.x}
    y={rowMenu.y}
    label={t('Row actions')}
    onselect={(id) => onRowMenu(target.project, target.tab, id, target.session)}
    onclose={() => (rowMenu = null)}
  />
{/if}

{#if menu}
  {@const target = menu.project}
  <Menu
    items={menuItems(target)}
    x={menu.x}
    y={menu.y}
    label={t('Project actions')}
    onselect={(id) => void onMenu(target, id)}
    onclose={() => (menu = null)}
  />
{/if}

<style>
  .rail {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    width: var(--k-rail-width);
    flex: none;
    min-height: 0;
    padding: var(--k-space-2) var(--k-space-3) var(--k-space-3);
    background: var(--k-rail);
  }

  button {
    font: inherit;
    color: inherit;
  }

  /* The label never gives way to its chord badge: the badge wraps onto a hidden second line (#362). */
  .settings {
    flex-wrap: wrap;
    align-content: flex-start;
    overflow: hidden;
  }

  .lead {
    display: flex;
    align-items: center;
    gap: inherit;
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
  }

  /* Queue's `⌘0` and Settings' `⌘,` sit on the right (#451). */
  .kbd {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: 100%;
  }

  /* Global views (44px, name over status word); work and session rows are 30px, one line. */
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 100%;
    height: var(--k-sidebar-row-height);
    flex: none;
    padding: 0 10px;
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg);
    cursor: pointer;
    text-align: left;
    transition: background-color var(--k-duration-fast) var(--k-ease);
  }

  /* Hover layers on top of whatever the row wears (selected, lit) instead of replacing it. */
  .item:hover {
    background-image: linear-gradient(var(--k-bg-hover), var(--k-bg-hover));
  }

  .item:focus-visible,
  .caption:focus-visible {
    outline: 2px solid var(--k-focus);
    outline-offset: -2px;
  }

  .item.active {
    background-color: var(--k-bg-selected);
  }

  .place.active {
    box-shadow: inset 3px 0 0 var(--k-accent);
  }

  .tile {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 24px;
    height: 24px;
    color: var(--k-fg-chrome);
  }

  .text {
    display: flex;
    flex-direction: column;
    justify-content: center;
    flex: 1;
    min-width: 0;
    line-height: 1.3;
  }

  .name,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: var(--k-font-size);
    font-weight: var(--k-weight-medium);
  }

  .place .name {
    font-weight: var(--k-weight-strong);
  }

  .sub {
    margin-top: -2px;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
  }

  /* Muted counts, no red (#430): the sub line says it; only the compact rail shows the number. */
  .badge {
    display: none;
    flex: none;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 10px;
    background: var(--k-bg-selected);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    font-weight: var(--k-weight-medium);
    line-height: 20px;
    text-align: center;
  }

  .group {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }

  .heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--k-space-3);
    flex: none;
    padding-left: var(--k-space-3);
  }

  h2 {
    margin: 0;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-medium);
  }

  .scroll {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
  }

  .band {
    display: flex;
    flex-direction: column;
    flex: none;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    position: relative;
    flex: none;
    border-radius: var(--k-radius);
  }

  .row.over {
    box-shadow: 0 -2px 0 0 var(--k-accent);
  }

  /* A dragged pane lands in this tab, or in a new tab of this band. */
  .row.pane-over,
  .band.pane-target {
    border-radius: var(--k-radius);
    box-shadow: inset 0 0 0 2px var(--k-accent);
  }

  /* Band caption: the project's name, its menu and its drag handle. No lamp: its rows carry them.
     No rules between bands: 8px margin plus the 2px `.scroll` gap put 10px above each caption (#451). */
  .caption-row {
    display: flex;
    align-items: center;
    height: 22px;
    margin-top: 8px;
  }

  .caption {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 10px;
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-muted);
    cursor: pointer;
    text-align: left;
  }

  /* [colour mark][name][row count] (#435). */
  .caption {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .caption .name,
  .tools-heading {
    font-size: var(--k-font-size-xs);
    font-weight: var(--k-weight-strong);
  }

  .caption .name {
    flex: 1;
    min-width: 0;
  }

  .mark {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--project-color);
  }

  .count {
    flex: none;
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
    font-weight: 400;
  }

  .caption:hover {
    color: var(--k-fg);
  }

  /* Other Claude sessions: its own section after the projects (#402), headed like a band. */
  .tools-heading {
    height: 22px;
    margin-top: 10px;
    padding: 0 10px;
    line-height: 22px;
  }

  /* Rows: [lamp 14px][key or glyph][title][stage], a short project-colour tick on the left (#451). */
  .tab-row,
  .instance {
    gap: var(--k-space-3);
    height: var(--k-rail-item);
    padding: 0 10px 0 12px;
  }

  .tab-row::before {
    content: '';
    position: absolute;
    top: 7px;
    bottom: 7px;
    left: 3px;
    width: 2px;
    border-radius: 2px;
    background: var(--project-color);
    opacity: 0.6;
  }

  .tab-row.active .name {
    font-weight: var(--k-weight-strong);
  }

  /* Lit row (§6.2): a session here waits on you. The selected row keeps its face; the lamp speaks. */
  .tab-row.lit:not(.active) {
    --k-fg-subtle: var(--k-fg-muted); /* only fg and fg-muted read on lit (§4.4) */
    background-color: var(--k-lit);
  }

  .tab-row .name {
    font-size: 13.5px;
  }

  .instance .name {
    font-size: var(--k-font-size-sm);
  }

  /* Background (#452): pinned above the footer, scrolls on its own past 40% of the sidebar. */
  .background-section {
    display: flex;
    flex-direction: column;
    flex: none;
    max-height: 40%;
    overflow-y: auto;
    scrollbar-width: thin;
  }

  .tools-heading.quiet {
    color: var(--k-fg-subtle);
    font-weight: 400;
  }

  /* The quiet session row (`N more`, `N shells`): muted, terminal glyph, no chevron. */
  .background,
  .sess {
    color: var(--k-fg-muted);
  }

  .tab-row.sess .name {
    font-size: 12.5px;
    font-weight: 400;
  }

  .socket {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 14px;
  }

  .glyph {
    font-size: var(--k-font-size-sm);
    line-height: 1;
  }

  .kind {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 16px;
    color: var(--k-fg-chrome);
  }

  .key,
  .stage {
    flex: none;
    font-size: var(--k-font-size-xs);
  }

  .key {
    color: var(--k-info);
    font-family: var(--k-font-mono);
  }

  .stage {
    color: var(--k-fg-subtle);
  }

  .index {
    flex: none;
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
  }

  .instance {
    padding-left: calc(12px + 14px + var(--k-space-3));
    color: var(--k-fg-muted);
  }

  /* "⋯" and "×" show on hover and keyboard focus. */
  .row :global(.close) {
    position: absolute;
    top: calc((var(--k-rail-item) - var(--k-control-height-sm)) / 2);
    right: 2px;
    visibility: hidden;
  }

  /* The caption's `⋯` sits over its count, so the count keeps the right edge (#451). */
  .caption-row :global(.more) {
    position: absolute;
    top: calc((22px - var(--k-control-height-sm)) / 2);
    right: 2px;
    visibility: hidden;
  }

  .row:is(:hover, :focus-within) :global(:is(.close, .more)) {
    visibility: visible;
  }

  .row:has(:global(.close)):is(:hover, :focus-within) :is(.index, .stage),
  .caption-row:is(:hover, :focus-within) .count {
    visibility: hidden;
  }

  .none {
    margin: 0;
    padding: var(--k-space-3);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .footer {
    display: flex;
    align-items: center;
    gap: var(--k-space-1);
    flex: none;
  }

  .settings {
    flex: 1;
    min-width: 0;
    height: 32px;
    color: var(--k-fg-chrome);
  }

  .settings .tile {
    width: 16px;
    height: 16px;
  }

  .settings .name {
    font-size: var(--k-font-size-sm);
    font-weight: 400;
  }

  .settings:hover {
    color: var(--k-fg);
  }

  /* The right edge: drag it, or ←/→ (Shift: bigger steps), Enter collapses. */
  .edge {
    position: absolute;
    top: 0;
    bottom: 0;
    right: calc(var(--k-gutter-hit) / -2);
    z-index: 3;
    width: var(--k-gutter-hit);
    cursor: col-resize;
    touch-action: none;
  }

  .edge::after {
    content: '';
    position: absolute;
    inset: 0 auto 0 50%;
    width: 2px;
    transform: translateX(-50%);
    transition: background var(--k-duration-fast) var(--k-ease);
  }

  .edge:hover::after,
  .edge.dragging::after,
  .edge:focus-visible::after {
    background: var(--k-accent);
  }

  .edge:focus-visible {
    outline: none;
  }

  /* Compact sidebar (collapsed, or a window below 1400px): a coloured rule per project, one tile per
     row. Labels stay in the accessibility tree and the tooltips. */
  .rail.compact {
    width: var(--k-rail-width-compact);
  }

  .compact .item {
    justify-content: center;
    padding: 0;
  }

  /* A row with no lamp (the socket renders nothing) shows a dim dot, so no tile reads as empty (#446). */
  .compact .socket:empty::before {
    content: '';
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--k-fg-muted);
    opacity: 0.5;
  }

  .compact .tab-row {
    gap: 2px;
    height: 28px;
    padding: 0 2px 0 4px;
  }

  .compact .lead {
    flex: none;
  }

  .compact .footer {
    flex-direction: column;
  }

  .compact .settings {
    flex: none;
    width: 100%;
  }

  .compact .heading {
    justify-content: center;
    padding-left: 0;
  }

  .compact .text,
  .compact .kbd,
  .compact h2,
  .compact .caption .name,
  .compact .index,
  .compact .key,
  .compact .stage,
  .compact .heading :global(.label),
  .compact .none {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .compact .caption-row {
    height: auto;
    margin-top: 0;
  }

  .compact .caption {
    position: relative;
    height: 9px;
    padding: 0;
  }

  .compact .caption::before {
    content: '';
    position: absolute;
    inset: 4px var(--k-space-2) auto;
    height: 1px;
    background: var(--project-color);
  }

  .compact .instance,
  .compact .fold-row,
  .compact .mark,
  .compact .count,
  .compact .row :global(.close),
  .compact .caption-row :global(.more) {
    display: none;
  }

  .compact .heading :global(.k-button) {
    width: var(--k-control-height-sm);
    padding: 0;
    border-color: var(--k-border-strong);
  }

  .compact .badge {
    position: absolute;
    top: 2px;
    left: calc(50% + 4px);
  }

  .compact .badge {
    display: block;
  }
</style>
