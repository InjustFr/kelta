<script lang="ts">
  // The context bar above the panes (#248, DESIGN §6.3): `‹` back, the breadcrumb of the selected rail
  // row (a drag region), what that row is about, and the `+` menu. Custom decorations: it is also the
  // window's title bar (macOS traffic-light inset, or Kelta's own controls and resize handles).
  // Native decorations: the breadcrumb is the window title.
  import { onMount } from 'svelte';

  import { tabHeaderRegistry, type AnyComponent } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { appInfo } from '$lib/ipc';
  import { GLOBAL, layout, projects, sessions, toasts, tools, ui, work, type GlobalView } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import { Button, currentPlatform, Icon, IconButton, Kbd, Menu, type MenuItem } from '$lib/ui';

  import { workTitle } from '../views/work/live';
  import { workKey } from '../views/work/phase';
  import { backTarget, type JumpLoc } from './jumplist';
  import { chordFor } from './labels';
  import { lazyComponents } from './lazy.svelte';
  import {
    activateProject,
    currentTab,
    focusedPane,
    focusedSession,
    isToolTab,
    rowLabel,
    selectRow,
    toolSubLines,
    VIEW_NAMES,
  } from './nav';

  type Dir = 'North' | 'South' | 'East' | 'West' | 'NorthEast' | 'NorthWest' | 'SouthEast' | 'SouthWest';
  const HANDLES: Dir[] = [
    'North',
    'South',
    'East',
    'West',
    'NorthEast',
    'NorthWest',
    'SouthEast',
    'SouthWest',
  ];
  const VIEW_ICONS = { tickets: 'ticket', now: 'inbox', reviews: 'git-pull-request' } as const;

  let custom = $state(false);
  const mac = currentPlatform() === 'macos';

  onMount(() => {
    appInfo()
      .then((info) => (custom = info.decorations === 'custom'))
      .catch(() => {});
  });

  // ---- what is selected ------------------------------------------------------------------------

  const view = $derived(ui.view);
  const project = $derived(projects.active);
  const tab = $derived(project ? currentTab(project.id) : null);
  const pane = $derived(project && tab ? focusedPane(project.id) : null);
  const session = $derived(project && tab ? focusedSession(project.id) : null);
  const isHome = $derived(!!project && project.id === projects.home?.id);
  const lines = $derived(toolSubLines());
  const toolId = $derived(tab && pane && isToolTab(tab, lines) ? (lines.get(pane.id)?.tool ?? null) : null);
  const tool = $derived(toolId ? tools.find(toolId) : null);
  const item = $derived(tab?.work_item_id ? work.get(tab.work_item_id) : null);
  /** A work tab is shown: the bar carries its work segment. */
  const workTab = $derived(!view && !tool && !!item);
  const ownerItem = $derived(session?.work_item_id ? work.get(session.work_item_id) : null);
  const projectColor = $derived(resolveProjectColor(project?.color) ?? 'var(--k-border-strong)');

  /** The row part of the breadcrumb for a plain tab: the focused session, else the tab title. */
  const row = $derived.by((): { glyph: string; text: string } | null => {
    if (!tab) return null;
    if (session?.kind.type === 'claude') return { glyph: '✻', text: session.name };
    if (session) return { glyph: '❯', text: session.title || session.name };
    return { glyph: '', text: rowLabel(tab) };
  });

  // shortcut: the home folder is guessed from the path (/Users/x, /home/x); use app_info once it says.
  const tilde = (p: string): string => p.replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, '~');
  const cwd = $derived(session ? tilde(session.cwd) : null);
  const model = $derived(session?.claude?.model ?? null);

  const here = $derived.by((): JumpLoc | null => {
    if (view) return { project: GLOBAL, tab: view, pane: view };
    return project && tab && pane ? { project: project.id, tab: tab.id, pane: pane.id } : null;
  });
  const back = $derived(backTarget(here));

  function placeLabel(loc: JumpLoc): string {
    if (loc.project === GLOBAL) return VIEW_NAMES[loc.tab as GlobalView];
    const t = layout.get(loc.project)?.tabs.find((x) => x.id === loc.tab);
    return `${projects.byId(loc.project)?.name ?? loc.project} › ${t ? rowLabel(t) : ''}`;
  }

  /** The breadcrumb as text: the window title in native mode. */
  const title = $derived.by(() => {
    if (view) return VIEW_NAMES[view];
    if (!project) return 'Kelta';
    if (tool) return `Tools › ${tool.label}`;
    if (item) return `${project.name} › ${workKey(item)} ${workTitle(item)}`;
    return row ? `${project.name} › ${row.text}` : project.name;
  });

  $effect(() => {
    const t = title;
    document.title = t;
    // Outside a Tauri window getCurrentWindow() throws; the catch swallows it.
    if (custom) return;
    import('@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => getCurrentWindow().setTitle(t))
      .catch(() => {});
  });

  // ---- the focused session's Claude workflow run (#272) -----------------------------------------

  const run = $derived.by(() => {
    const list = session ? sessions.runs[session.id] : undefined;
    return list?.findLast((r) => r.status === 'running') ?? list?.at(-1) ?? null;
  });
  /** `<workflow> · <phase with a running agent>`, or its final status. */
  const runChip = $derived.by(() => {
    if (!run) return null;
    const phase = run.phases.findLast((p) => p.agents.some((a) => !a.done)) ?? run.phases.at(-1);
    return `${run.name ?? 'workflow'} · ${run.status === 'running' ? (phase?.title ?? 'starting') : run.status}`;
  });
  $effect(() => {
    if (session?.kind.type === 'claude') void sessions.loadRuns(session.id);
  });

  // ---- the work item segment (lazy, like the rest of views/work) -------------------------------

  const WorkComponent = $derived(lazyComponents.get('tabheader:work').component as AnyComponent | null);
  $effect(() => {
    if (workTab) lazyComponents.ensure('tabheader:work', tabHeaderRegistry.work);
  });

  // ---- actions ---------------------------------------------------------------------------------

  /** A tool's context chip: the tab of its work item, else the project's first row. */
  function goOwner(): void {
    if (!project) return;
    const tabs = layout.get(project.id)?.tabs ?? [];
    const t =
      (ownerItem && tabs.find((x) => x.work_item_id === ownerItem.id)) ??
      tabs.find((x) => !isToolTab(x, lines));
    if (t) selectRow(project.id, t.id);
  }

  let plusMenu = $state<{ x: number; y: number } | null>(null);
  const PLUS: MenuItem[] = [
    { id: 'session', label: 'New session…', icon: 'square-terminal', kbd: chordFor('session.new') },
    { id: 'tickets', label: 'Tickets', icon: 'ticket', kbd: chordFor('tickets.open') },
    { id: 'reviews', label: 'Reviews', icon: 'git-pull-request', kbd: chordFor('reviews.open') },
  ];

  async function onPlus(id: string): Promise<void> {
    // The project's own tabs; with no project shown, the bare actions open the rail's cross-project views.
    const args = project && !view ? { project_id: project.id } : undefined;
    try {
      if (id === 'session') ui.openSheet('session_new');
      else if (id === 'tickets') await dispatch('tickets.open', args);
      else if (id === 'reviews') await dispatch('reviews.open', args);
    } catch (err) {
      toasts.error(err, 'Could not open the view');
    }
  }

  async function win(op: 'minimize' | 'toggleMaximize' | 'close'): Promise<void> {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow()[op]();
  }

  async function resize(e: PointerEvent, dir: Dir): Promise<void> {
    if (e.button !== 0) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().startResizeDragging(dir);
  }
</script>

<header
  class="contextbar"
  class:inset={custom && mac}
  data-tauri-drag-region
  data-testid="contextbar"
  data-custom={custom || undefined}
>
  <IconButton
    icon="chevron-left"
    size="sm"
    label={back ? `Back to ${placeLabel(back)}` : 'Back'}
    chord={chordFor('nav.back')}
    data-testid="context-back"
    onclick={() => void dispatch('nav.back')}
  />

  <span class="crumb" data-tauri-drag-region data-testid="context-crumb">
    {#if view}
      <span class="here" data-tauri-drag-region
        ><Icon name={VIEW_ICONS[view]} size={14} /> {VIEW_NAMES[view]}</span
      >
    {:else if tool}
      <span class="parent" data-tauri-drag-region>Tools</span>
      <span class="sep" aria-hidden="true">›</span>
      <span class="here" data-tauri-drag-region><span aria-hidden="true">⚒</span>{tool.label}</span>
    {:else if project}
      <span class="project" style:--project-color={projectColor} data-tauri-drag-region>{project.name}</span>
      {#if item}
        <span class="sep" aria-hidden="true">›</span>
        <span class="key" data-testid="context-key" data-tauri-drag-region>{workKey(item)}</span>
        <span class="ttl" data-tauri-drag-region>{workTitle(item)}</span>
      {:else if row}
        <span class="sep" aria-hidden="true">›</span>
        <span class="here" data-tauri-drag-region
          >{#if row.glyph}<span aria-hidden="true">{row.glyph}</span>{/if}{row.text}</span
        >
      {/if}
    {/if}
  </span>

  {#if workTab}
    <!-- The work segment brings its own spacer and right-hand actions. -->
    {#if WorkComponent && project && tab && item}
      <WorkComponent projectId={project.id} workItemId={item.id} />
    {/if}
  {:else}
    {#if tool && project}
      {#if isHome}
        <span class="muted" data-tauri-drag-region>for you</span>
      {:else}
        <Button
          size="sm"
          variant="ghost"
          class="chip owner"
          title="Go to {project.name}"
          data-testid="context-owner"
          onclick={goOwner}
          ><span class="project" style:--project-color={projectColor}>{project.name}</span
          >{#if ownerItem}<span class="sep">›</span><span class="key">{workKey(ownerItem)}</span>{/if}</Button
        >
        {#if cwd}<span class="meta" title={session?.cwd} data-tauri-drag-region>{cwd}</span>{/if}
      {/if}
    {/if}
    <span class="spacer" data-tauri-drag-region></span>
    {#if view && project}
      <Button
        size="sm"
        variant="ghost"
        class="chip"
        chord={chordFor('nav.back')}
        title="Leave {VIEW_NAMES[view]} for what is underneath"
        data-testid="context-underneath"
        onclick={() => void activateProject(project.id)}
        >‹ {project.name}{tab ? ` › ${rowLabel(tab)}` : ''}</Button
      >
    {:else if tool}
      {#if back}
        <Button
          size="sm"
          variant="ghost"
          class="chip"
          title="Back to {placeLabel(back)}"
          data-testid="context-return"
          onclick={() => void dispatch('nav.back')}
          >↩ {placeLabel(back)}{#if tool.keybinding}
            <Kbd chord={tool.keybinding} />{/if}</Button
        >
      {/if}
    {:else if cwd}
      <span class="meta" title={session?.cwd} data-testid="context-meta" data-tauri-drag-region
        >{cwd}{#if model}<span class="sep">·</span>{model}{/if}</span
      >
    {/if}
  {/if}

  {#if runChip && session}
    {@const sid = session.id}
    <Button
      size="sm"
      variant="ghost"
      class="chip"
      title="Workflow runs of this session"
      data-testid="context-workflow"
      onclick={() => ui.openSheet('claude_runs', { sessionId: sid })}
      ><span aria-hidden="true">⚙</span> {runChip}</Button
    >
  {/if}

  <Button
    variant="ghost"
    size="sm"
    icon="plus"
    class="plus"
    aria-label="New tab"
    title="New tab"
    data-testid="tab-plus"
    onclick={(e) => {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      plusMenu = { x: r.right - 220, y: r.bottom + 2 };
    }}
  />

  {#if custom && !mac}
    <span class="controls">
      <IconButton icon="minus" label="Minimize" size="sm" onclick={() => void win('minimize')} />
      <IconButton icon="square" label="Maximize" size="sm" onclick={() => void win('toggleMaximize')} />
      <IconButton icon="x" label="Close window" size="sm" onclick={() => void win('close')} />
    </span>
  {/if}
</header>

{#if custom && !mac}
  {#each HANDLES as dir (dir)}
    <div class="handle {dir}" aria-hidden="true" onpointerdown={(e) => void resize(e, dir)}></div>
  {/each}
{/if}

{#if plusMenu}
  <Menu
    items={PLUS}
    x={plusMenu.x}
    y={plusMenu.y}
    label="Open"
    onselect={(id) => void onPlus(id)}
    onclose={() => (plusMenu = null)}
  />
{/if}

<style>
  .contextbar {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: var(--k-contextbar-height);
    flex: none;
    padding: 0 var(--k-space-2);
    container: contextbar / inline-size;
    background: var(--k-bezel);
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-sm);
    white-space: nowrap;
    user-select: none;
  }

  /* macOS custom chrome: the system traffic lights sit over this inset. */
  .contextbar.inset {
    padding-left: 72px;
  }

  .contextbar :global(.k-button),
  .contextbar :global(.k-icon-button) {
    flex: none;
    height: var(--k-control-height-sm);
  }

  /* ▌project › row: the project colour bar, then where you are. */
  .crumb {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    flex: 0 1 auto;
    min-width: 0;
  }

  .project {
    flex: none;
    padding-left: var(--k-space-3);
    box-shadow: inset 3px 0 0 var(--project-color);
    color: var(--k-fg-muted);
    font-weight: var(--k-weight-medium);
  }

  .parent {
    flex: none;
    color: var(--k-fg-muted);
  }

  .sep {
    color: var(--k-fg-subtle);
  }

  .key {
    flex: none;
    color: var(--k-fg-muted);
    font-family: var(--k-font-mono);
  }

  .here,
  .ttl {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
  }

  .ttl {
    display: block;
  }

  .meta,
  .muted {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--k-fg-muted);
  }

  .meta .sep {
    margin: 0 var(--k-space-2);
  }

  :global(.owner) .project {
    padding-left: var(--k-space-2);
  }

  :global(.owner) .sep {
    margin: 0 var(--k-space-1);
  }

  .spacer {
    flex: 1;
    align-self: stretch;
  }

  .controls {
    display: inline-flex;
    flex: none;
    gap: var(--k-space-1);
  }

  /* Narrow bars (#248): the cwd and model go first, the work title is the 6th step (the rest in WorkContext). */
  @container contextbar (max-width: 1280px) {
    .meta {
      display: none;
    }
  }

  @container contextbar (max-width: 840px) {
    .ttl {
      display: none;
    }
  }

  .handle {
    position: fixed;
    z-index: var(--k-z-toast);
  }
  .North,
  .South {
    left: 8px;
    right: 8px;
    height: 4px;
    cursor: ns-resize;
  }
  .East,
  .West {
    top: 8px;
    bottom: 8px;
    width: 4px;
    cursor: ew-resize;
  }
  .North {
    top: 0;
  }
  .South {
    bottom: 0;
  }
  .East {
    right: 0;
  }
  .West {
    left: 0;
  }
  .NorthEast,
  .NorthWest,
  .SouthEast,
  .SouthWest {
    width: 8px;
    height: 8px;
  }
  .NorthWest {
    top: 0;
    left: 0;
    cursor: nwse-resize;
  }
  .SouthEast {
    bottom: 0;
    right: 0;
    cursor: nwse-resize;
  }
  .NorthEast {
    top: 0;
    right: 0;
    cursor: nesw-resize;
  }
  .SouthWest {
    bottom: 0;
    left: 0;
    cursor: nesw-resize;
  }
</style>
