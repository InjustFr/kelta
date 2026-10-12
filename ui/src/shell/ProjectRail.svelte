<script lang="ts">
  import { t, tn } from '$lib/i18n/index.svelte';
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import type { ProjectId, ProjectInfo, Tab, ToolInfo } from '$lib/gen';
  import { allPanes, moveTab } from '$lib/layout';
  import {
    layout,
    projects,
    reviews,
    sessions,
    tickets,
    toasts,
    tools,
    ui,
    type GlobalView,
  } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import { Button, Icon, IconButton, isIconName, Kbd, Lamp, Menu, type MenuItem } from '$lib/ui';
  import { currentPlatform, formatChord } from '$lib/ui/format';
  import { frames } from '$lib/terminal/raf';

  import { jumpOutside, nowSummary, refreshNow } from '../views/inbox/now';
  import { outsidePhase } from '../views/work/phase';
  import { confirms } from './confirm.svelte';
  import { chordFor, currentRun, runState, toolIcon } from './labels';
  import {
    activateProject,
    attachTab,
    backgroundSessions,
    currentTab,
    detachTab,
    focusedPane,
    focusPaneById,
    isToolTab,
    liveSessionsOfTab,
    openView,
    railProjects,
    railRows,
    railTools,
    railToolsOf,
    requestCloseTab,
    revealSession,
    rowLabel,
    selectRow,
    tabAttention,
    toolLamp,
    toolSubLines,
    VIEW_NAMES,
    workItemOfTab,
    type ToolInstance,
  } from './nav';
  import { droppedOutside, paneDrag } from './paneDrag.svelte';
  import { peekHover, peekTarget } from './peek.svelte';
  import { SIDEBAR_MAX, SIDEBAR_MIN, sidebar } from './sidebar.svelte';

  const rail = $derived(railProjects());
  const reorderable = $derived(rail.filter((p) => !p.builtin));

  // Now's tile: badge = what waits on Louis (first four sections), sub line = the most urgent chip (#386).
  const now = $derived(nowSummary());
  // Tickets and Reviews counts are muted: the red badge stays on Now, so nothing is counted twice.
  const ALL = { kind: 'all' } as const;
  const mine = $derived(
    tickets.items(ALL, null, 'mine').filter((i) => i.ticket.status.category !== 'done').length,
  );
  const toReview = $derived(reviews.list(ALL, 'review_requested').data?.items.length ?? 0);

  const toolsOf = $derived(railToolsOf());
  const toolList = $derived(railTools());
  // Tool membership, once per change: the Tools sub-lines and which tabs leave the bands.
  const lines = $derived(toolSubLines());
  // `project.goto.N` index of each row.
  const index = $derived(new Map(railRows(lines).map((r, i) => [r.tab.id, i + 1])));
  $effect(() => {
    if (tools.byProject[toolsOf] === undefined) void tools.load(toolsOf);
  });

  // Each session's current Claude workflow run (#377); a closed session drops its runs.
  const runs = $derived(
    Object.entries(sessions.runs).flatMap(([id, list]) => {
      const s = sessions.get(id);
      const run = currentRun(list);
      return s && run ? [{ s, run, ...runState(run, s) }] : [];
    }),
  );
  // Runs a Claude session started before this window listened (once per session; events keep them current).
  $effect(() => {
    for (const s of sessions.all)
      if (s.kind.type === 'claude' && s.lifecycle === 'live') void sessions.loadRuns(s.id);
  });

  // The pane in front (no global view): marks the tool row or sub-line it belongs to.
  const front = $derived(ui.view ? null : focusedPane());
  const frontTab = $derived(ui.view ? null : currentTab());
  const frontTool = $derived(front ? (lines.get(front.id)?.tool ?? null) : null);

  // Startup and window focus refresh Now's sources (no polling).
  onMount(() => void refreshNow());

  let menu = $state<{ project: ProjectInfo; x: number; y: number } | null>(null);
  let rowMenu = $state<{ project: ProjectId; tab: Tab; x: number; y: number } | null>(null);
  let dragId = $state<string | null>(null);
  let overId = $state<string | null>(null);
  let rowDrag = $state<{ project: ProjectId; tab: string } | null>(null);
  let rowOver = $state<string | null>(null);
  // A pane dragged over a band but not over one of its rows: dropping opens it in a new tab.
  let bandOver = $state<ProjectId | null>(null);

  function menuItems(p: ProjectInfo): MenuItem[] {
    const index = reorderable.findIndex((x) => x.id === p.id);
    const items: MenuItem[] = [{ id: 'settings', label: t('Project settings…'), icon: 'settings' }];
    if (!p.builtin) {
      items.push(
        { id: 'up', label: t('Move up'), icon: 'arrow-up', disabled: index <= 0, separator: true },
        { id: 'down', label: t('Move down'), icon: 'arrow-down', disabled: index >= reorderable.length - 1 },
        { id: 'close', label: t('Close project'), icon: 'x', separator: true },
        { id: 'close_kill', label: t('Close and stop sessions'), icon: 'square', danger: true },
        { id: 'remove', label: t('Remove project…'), icon: 'trash-2', danger: true },
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

  /** The band's rows: the project's non-tool tabs, in tab order. */
  function rowsOf(id: ProjectId): Tab[] {
    return (layout.get(id)?.tabs ?? []).filter((x) => !isToolTab(x, lines));
  }

  /** Moves a row to `target`'s place; rows move only inside their band. */
  function moveRow(project: ProjectId, tabId: string, target: Tab | undefined): void {
    const to = target ? (layout.get(project)?.tabs.indexOf(target) ?? -1) : -1;
    if (to >= 0) layout.update(project, (l) => moveTab(l, tabId, to));
  }

  function rowMenuItems(project: ProjectId, tab: Tab): MenuItem[] {
    const rows = rowsOf(project);
    const index = rows.findIndex((x) => x.id === tab.id);
    return [
      { id: 'close', label: t('Close'), icon: 'x' },
      tab.detached
        ? { id: 'attach', label: t('Move back to main window'), icon: 'app-window' }
        : { id: 'detach', label: t('Move to new window'), icon: 'app-window' },
      { id: 'up', label: t('Move up'), icon: 'arrow-up', disabled: index <= 0, separator: true },
      { id: 'down', label: t('Move down'), icon: 'arrow-down', disabled: index >= rows.length - 1 },
    ];
  }

  function onRowMenu(project: ProjectId, tab: Tab, id: string): void {
    const rows = rowsOf(project);
    const index = rows.findIndex((t) => t.id === tab.id);
    if (id === 'close') void requestCloseTab(project, tab.id);
    else if (id === 'detach') void detachTab(project, tab.id);
    else if (id === 'attach') void attachTab(project, tab.id);
    else moveRow(project, tab.id, rows[index + (id === 'up' ? -1 : 1)]);
  }

  function onRowDrop(project: ProjectId, target: Tab): void {
    if (paneDrag.current) {
      rowOver = null;
      paneDrag.drop(project, { tabId: target.id });
      return;
    }
    const from = rowDrag;
    rowDrag = null;
    rowOver = null;
    if (from && from.project === project && from.tab !== target.id) moveRow(project, from.tab, target);
  }

  function rowIcon(tab: Tab): string {
    if (tab.detached) return 'app-window';
    if (tab.work_item_id) return 'git-branch';
    if (liveSessionsOfTab(tab).some((s) => s.kind.type === 'claude')) return 'bot';
    return allPanes(tab.root).some((p) => p.content.kind === 'terminal') ? 'terminal' : 'app-window';
  }

  function isFront(project: ProjectId, tabId: string): boolean {
    return !ui.view && projects.activeId === project && layout.get(project)?.active_tab === tabId;
  }

  function instanceLabel(x: ToolInstance): string {
    return workItemOfTab(x.tab)?.ticket?.key ?? projects.byId(x.project)?.name ?? x.project;
  }

  function focusInstance(x: ToolInstance): void {
    void activateProject(x.project);
    focusPaneById(x.project, x.tab.id, x.pane.id);
  }

  /** `label (Ctrl+Shift+K)`: the tooltip keeps the chord when its badge has no room (#362). */
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

  function toolHint(tool: ToolInfo): string {
    if (tool.installed === false) return t('{tool}: not installed', { tool: tool.label });
    return tool.description ? `${tool.label}: ${tool.description}` : tool.label;
  }
</script>

<svelte:window onfocus={() => void refreshNow()} />

{#snippet chord(id: string)}
  {@const c = chordFor(id)}
  {#if c}<span class="kbd" aria-hidden="true"><Kbd chord={c} /></span>{/if}
{/snippet}

<!-- A global view (SPEC §1): shown outside any project; the active project stays as it is. -->
{#snippet place(view: GlobalView, icon: string, id: string, sub: string, n: number, muted: boolean)}
  {@const name = VIEW_NAMES[view]}
  <button
    type="button"
    class="item place"
    class:active={ui.view === view}
    onclick={() => openView(view)}
    title={`${name}: ${sub}`}
    aria-current={ui.view === view ? 'page' : undefined}
    data-testid={`rail-${id}`}
  >
    <span class="tile plain"><Icon name={icon} size={18} /></span>
    <span class="text">
      <span class="name">{name}</span>
      <span class="sub">{sub}</span>
    </span>
    {#if n > 0}
      <span class="badge k-num" class:muted data-testid={`${id}-badge`}>{n > 99 ? '99+' : n}</span>
    {/if}
  </button>
{/snippet}

<!-- The muted `⌘N` / `⌥⌘N` index of a row, when that chord is bound. -->
{#snippet digit(id: string, text: string)}
  {@const c = chordFor(id)}
  {#if c}<span class="index k-num" title={formatChord(c).join('')} aria-hidden="true">{text}</span>{/if}
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

<nav
  class="rail"
  class:compact={sidebar.compact}
  aria-label={t('Projects')}
  data-testid="rail"
  style:--k-rail-width="{sidebar.width}px"
  bind:this={nav}
>
  <button
    type="button"
    class="search"
    title={tip(t('Search or run a command'), 'palette.open')}
    onclick={() => void dispatch('palette.open')}
    data-testid="rail-search"
  >
    <span class="lead"
      ><Icon name="search" size={16} /><span class="search-text">{t('Search or run…')}</span></span
    >
    {@render chord('palette.open')}
  </button>

  {@render place('tickets', 'ticket', 'tickets', t('{n} mine', { n: mine }), mine, true)}
  {@render place('now', 'inbox', 'inbox', now.top, now.waiting, false)}
  {@render place(
    'reviews',
    'git-pull-request',
    'reviews',
    t('{n} to review', { n: toReview }),
    toReview,
    true,
  )}

  <div class="group">
    <div class="heading">
      <h2>{t('Open')}</h2>
      <Button
        variant="ghost"
        size="sm"
        icon="plus"
        title={t('Open or create a project')}
        onclick={() => ui.openSheet('project_new')}
        data-testid="rail-add">{t('Add project')}</Button
      >
    </div>

    <div class="scroll">
      {#each rail as p (p.id)}
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
              draggable={!p.builtin}
              onclick={() => activateProject(p.id)}
              oncontextmenu={(e) => openMenu(e, p)}
              ondragstart={(e) => {
                dragId = p.id;
                e.dataTransfer?.setData('text/plain', p.id);
                if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
              }}
              ondragover={(e) => {
                if (!dragId || p.builtin) return;
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
              }}><span class="name">{p.name}</span></button
            >
            {@render more(p)}
          </div>
          <ul class="rows">
            {#each rowsOf(p.id) as tab (tab.id)}
              {@const lvl = tabAttention(tab)}
              {@const active = isFront(p.id, tab.id)}
              {@const label = rowLabel(tab)}
              {@const over = rowOver === tab.id}
              <li
                class="row"
                class:over={over && rowDrag && rowDrag.tab !== tab.id}
                class:pane-over={over && paneDrag.current && paneDrag.current.tabId !== tab.id}
              >
                <button
                  type="button"
                  class="item tab-row"
                  class:active
                  class:lit={lvl === 'needs_input'}
                  title={`${p.name} › ${label}`}
                  aria-current={active ? 'page' : undefined}
                  data-testid="rail-row"
                  data-project-id={p.id}
                  data-tab-id={tab.id}
                  data-attention={lvl}
                  use:peekHover={() => peekTarget(liveSessionsOfTab(tab))}
                  draggable="true"
                  onclick={() => selectRow(p.id, tab.id)}
                  onauxclick={(e) => {
                    if (e.button !== 1) return;
                    e.preventDefault();
                    void requestCloseTab(p.id, tab.id);
                  }}
                  onmousedown={(e) => {
                    if (e.button === 1) e.preventDefault(); // no autoscroll on middle click
                  }}
                  oncontextmenu={(e) => {
                    e.preventDefault();
                    rowMenu = { project: p.id, tab, x: e.clientX, y: e.clientY };
                  }}
                  ondragstart={(e) => {
                    rowDrag = { project: p.id, tab: tab.id };
                    e.dataTransfer?.setData('text/plain', tab.id);
                  }}
                  ondragover={(e) => {
                    const pane = paneDrag.current;
                    if (pane ? pane.projectId !== p.id : rowDrag?.project !== p.id) return;
                    e.preventDefault();
                    rowOver = tab.id;
                  }}
                  ondragleave={() => (rowOver = null)}
                  ondrop={(e) => {
                    e.preventDefault();
                    onRowDrop(p.id, tab);
                  }}
                  ondragend={(e) => {
                    if (rowDrag && droppedOutside(e)) void detachTab(p.id, tab.id);
                    rowDrag = null;
                    rowOver = null;
                  }}
                >
                  <span class="socket" aria-hidden="true"><Lamp level={lvl} /></span>
                  <span class="kind" aria-hidden="true"><Icon name={rowIcon(tab)} size={14} /></span>
                  <span class="text"><span class="name">{label}</span></span>
                  {@render digit(`project.goto.${index.get(tab.id)}`, `${index.get(tab.id)}`)}
                </button>
                <IconButton
                  class="close"
                  size="sm"
                  icon="x"
                  label={t('Close {name}', { name: label })}
                  onclick={() => void requestCloseTab(p.id, tab.id)}
                />
              </li>
            {/each}
            {#each backgroundSessions(p.id) as s (s.id)}
              <li class="row">
                <button
                  type="button"
                  class="item tab-row background"
                  title={`${p.name} › ${t('{name}: running in the background, click to show it', { name: s.name })}`}
                  data-testid="rail-background"
                  data-session-id={s.id}
                  onclick={() => void revealSession(s.id)}
                >
                  <span class="socket" aria-hidden="true"></span>
                  <span class="kind" aria-hidden="true"
                    ><Icon name={s.kind.type === 'claude' ? 'bot' : 'terminal'} size={14} /></span
                  >
                  <span class="text"><span class="name">{s.name} · {t('background')}</span></span>
                </button>
              </li>
            {/each}
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

      {#if runs.length > 0}
        <section class="band tools" aria-label={t('Workflows')} data-testid="rail-workflows">
          <h2 class="tools-heading">{t('Workflows')}</h2>
          <ul class="rows">
            {#each runs as r (r.run.run_id)}
              <li class="row">
                <button
                  type="button"
                  class="item tab-row"
                  title={`${r.s.name} › ${t('Workflow runs of this session')}`}
                  data-testid="rail-workflow"
                  data-run-id={r.run.run_id}
                  data-attention={r.lamp}
                  onclick={() => ui.openSheet('claude_runs', { sessionId: r.s.id })}
                >
                  <span class="socket" aria-hidden="true"><Lamp level={r.lamp} /></span>
                  <span class="kind" aria-hidden="true"><Icon name="bot" size={14} /></span>
                  <span class="text"><span class="name">{r.run.name ?? t('workflow')} · {r.label}</span></span
                  >
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if sessions.elsewhere.length > 0}
        <!-- Claude sessions Kelta did not start (#345): dimmed, Now lists them. -->
        <section class="band tools" aria-label={t('Elsewhere')} data-testid="rail-elsewhere">
          <h2 class="tools-heading">{t('Elsewhere')}</h2>
          <ul class="rows">
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
          </ul>
        </section>
      {/if}

      {#if toolList.length > 0}
        <section class="band tools" aria-label={t('Tools')} data-testid="rail-tools">
          <h2 class="tools-heading">{t('Tools')}</h2>
          <ul class="rows">
            {#each toolList as tool, i (tool.id)}
              {@const launchOnly = tool.kind !== 'pty'}
              {@const lvl = launchOnly ? 'none' : toolLamp(tool.id)}
              {@const mark = toolIcon(tool)}
              <!-- A `user` tool has one instance: its row is the way to it. -->
              {@const instances =
                tool.scope === 'user' ? [] : [...lines.values()].filter((x) => x.tool === tool.id)}
              <li class="row">
                <button
                  type="button"
                  class="item tool-row"
                  class:active={frontTool === tool.id && !!frontTab && isToolTab(frontTab, lines)}
                  class:missing={tool.installed === false}
                  title={toolHint(tool)}
                  data-testid="rail-tool"
                  data-tool-id={tool.id}
                  data-attention={lvl}
                  onclick={() => void dispatch('tools.open', { tool_id: tool.id, focus_only: true })}
                >
                  <span class="socket" aria-hidden="true"><Lamp level={lvl} /></span>
                  <span class="kind" aria-hidden="true">
                    {#if isIconName(mark)}<Icon name={mark} size={14} />{:else}<span class="abbr">{mark}</span
                      >{/if}
                  </span>
                  <span class="text"><span class="name">{tool.label}</span></span>
                  {@render digit(`tool.goto.${i + 1}`, `⌥${i + 1}`)}
                  {#if launchOnly}<span class="launch" aria-hidden="true"
                      ><Icon name="external-link" size={12} /></span
                    >{/if}
                </button>
              </li>
              {#each instances as x (x.pane.id)}
                <li class="row">
                  <button
                    type="button"
                    class="item instance"
                    class:active={front?.id === x.pane.id && isFront(x.project, x.tab.id)}
                    title={t('{tool} in {place}', {
                      tool: tool.label,
                      place: `${projects.byId(x.project)?.name ?? x.project} › ${t(x.tab.title)}`,
                    })}
                    data-testid="rail-tool-instance"
                    data-tool-id={tool.id}
                    data-pane-id={x.pane.id}
                    onclick={() => focusInstance(x)}
                  >
                    <span class="text"><span class="name">└ {instanceLabel(x)}</span></span>
                  </button>
                </li>
              {/each}
            {/each}
          </ul>
        </section>
      {/if}
    </div>
  </div>

  <div class="footer">
    <button
      type="button"
      class="item settings"
      title={tip(t('Settings'), 'settings.open')}
      onclick={() => void dispatch('settings.open')}
      data-testid="rail-settings"
    >
      <span class="lead">
        <span class="tile plain"><Icon name="settings" size={18} /></span>
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
    items={rowMenuItems(target.project, target.tab)}
    x={rowMenu.x}
    y={rowMenu.y}
    label={t('Row actions')}
    onselect={(id) => onRowMenu(target.project, target.tab, id)}
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
    gap: var(--k-space-3);
    width: var(--k-rail-width);
    flex: none;
    min-height: 0;
    padding: var(--k-space-3);
    border-right: 1px solid var(--k-border);
    background: var(--k-bezel);
  }

  button {
    font: inherit;
    color: inherit;
  }

  /* The label never gives way to its chord badge: the badge wraps onto a hidden second line (#362). */
  .search,
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

  .search {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    flex: none;
    height: var(--k-control-height);
    padding: 0 var(--k-space-2) 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
    color: var(--k-fg-muted);
    cursor: pointer;
    text-align: left;
  }

  .search:hover {
    border-color: var(--k-border-strong);
    color: var(--k-fg);
  }

  .search-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: var(--k-font-size-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kbd {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: 100%;
  }

  /* Global views (44px, name over status word); session and tool rows are 32px, one line. */
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: var(--k-sidebar-row-height);
    flex: none;
    padding: 0 var(--k-space-3);
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg);
    cursor: pointer;
    text-align: left;
    transition: background-color var(--k-duration) ease-out;
  }

  /* Hover layers on top of whatever the row wears (selected, lit) instead of replacing it. */
  .item:hover {
    background-image: linear-gradient(var(--k-bg-hover), var(--k-bg-hover));
  }

  .item:focus-visible,
  .search:focus-visible,
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
    width: 28px;
    height: 28px;
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

  .sub {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
  }

  .badge {
    flex: none;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 10px;
    background: var(--k-lamp-needs-input);
    color: var(--k-well);
    font-size: var(--k-font-size-xs);
    font-weight: var(--k-weight-strong);
    line-height: 20px;
    text-align: center;
  }

  /* Muted counts (Tickets, Reviews): the sub line says it; the compact rail shows the number. */
  .badge.muted {
    display: none;
    background: var(--k-bg-selected);
    color: var(--k-fg-muted);
    font-weight: var(--k-weight-medium);
  }

  .group {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    margin-top: var(--k-space-2);
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
    gap: var(--k-space-2);
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
    gap: 1px;
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

  /* Band caption: the project's name, its menu and its drag handle. No lamp: its rows carry them. */
  .caption-row {
    display: flex;
    align-items: center;
  }

  .caption {
    flex: 1;
    min-width: 0;
    height: var(--k-control-height-sm);
    padding: 0 var(--k-space-3);
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-muted);
    cursor: pointer;
    text-align: left;
  }

  .caption .name {
    display: block;
    font-size: var(--k-font-size-sm);
  }

  .caption:hover {
    color: var(--k-fg);
  }

  .tools-heading {
    padding: 0 var(--k-space-3);
    line-height: var(--k-control-height-sm);
  }

  /* Rows: [lamp socket][glyph][title], the project colour bar on the left (full height when selected). */
  .tab-row,
  .tool-row,
  .instance {
    gap: var(--k-space-2);
    height: var(--k-rail-item);
    padding: 0 var(--k-space-2);
  }

  .tab-row {
    box-shadow: inset 2px 0 0 color-mix(in oklab, var(--project-color) 55%, transparent);
  }

  .tab-row.active {
    box-shadow: inset 3px 0 0 var(--project-color);
  }

  .tab-row.active .name,
  .tool-row.active .name {
    font-weight: var(--k-weight-strong);
  }

  /* Lit row (§6.2): a session here waits on you. The selected row keeps its face; the lamp speaks. */
  .tab-row.lit:not(.active) {
    --k-fg-subtle: var(--k-fg-muted); /* only fg and fg-muted read on lit (§4.4) */
    background-color: var(--k-lit);
  }

  .tab-row .name,
  .tool-row .name,
  .instance .name {
    font-size: var(--k-font-size-sm);
  }

  .background {
    color: var(--k-fg-muted);
  }

  .socket {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 10px;
  }

  .kind {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 16px;
    color: var(--k-fg-chrome);
  }

  .abbr {
    font-size: var(--k-font-size-xs);
    font-weight: var(--k-weight-strong);
  }

  .index {
    flex: none;
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
  }

  .launch {
    display: inline-flex;
    flex: none;
    color: var(--k-fg-muted);
  }

  .missing {
    opacity: 0.55;
  }

  .instance {
    padding-left: calc(var(--k-space-2) * 2 + 10px);
    color: var(--k-fg-muted);
  }

  /* "⋯" and "×" show on hover and keyboard focus. */
  .row :global(.close) {
    position: absolute;
    top: calc((var(--k-rail-item) - var(--k-control-height-sm)) / 2);
    right: 2px;
    visibility: hidden;
  }

  .caption-row :global(.more) {
    visibility: hidden;
  }

  .row:is(:hover, :focus-within) :global(:is(.close, .more)) {
    visibility: visible;
  }

  .row:has(:global(.close)):is(:hover, :focus-within) .index {
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
    padding-top: var(--k-space-3);
    border-top: 1px solid var(--k-border);
  }

  .settings {
    flex: 1;
    min-width: 0;
    height: 36px;
    color: var(--k-fg-chrome);
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
    transition: background var(--k-duration) ease-out;
  }

  .edge:hover::after,
  .edge.dragging::after,
  .edge:focus-visible::after {
    background: var(--k-accent);
  }

  .edge:focus-visible {
    outline: none;
  }

  /* Compact sidebar (collapsed, or a window below 1100px): a coloured rule per project, one tile per
     row. Labels stay in the accessibility tree and the tooltips. */
  .rail.compact {
    width: var(--k-rail-width-compact);
  }

  .compact .search {
    align-self: center;
    justify-content: center;
    width: 40px;
    padding: 0;
  }

  .compact .item {
    justify-content: center;
    padding: 0;
  }

  .compact .tab-row,
  .compact .tool-row {
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
  .compact .search-text,
  .compact .kbd,
  .compact h2,
  .compact .caption .name,
  .compact .launch,
  .compact .index,
  .compact .heading :global(.label),
  .compact .none {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
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

  .compact .tools {
    padding-top: var(--k-space-2);
    border-top: 1px solid var(--k-border);
  }

  .compact .instance,
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

  .compact .badge.muted {
    display: block;
  }
</style>
