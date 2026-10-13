<script lang="ts">
  // The window's top bar (#248, #429, DESIGN §6.3): `‹ ›` (the jumplist), the breadcrumb of the selected
  // rail row (a drag region; none on a work tab, whose header above its panes names it,
  // views/work/WorkContext.svelte, #428), the search field opening the palette, the project scope of the
  // global lists, the "needs you" chip and `+ New`. Custom decorations: it is also the window's title
  // bar (macOS traffic-light inset, or Kelta's own controls and resize handles). Native decorations: the
  // breadcrumb is the window title.
  import { t, tn } from '$lib/i18n/index.svelte';
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import { appInfo } from '$lib/ipc';
  import { GLOBAL, layout, projects, sessions, toasts, tools, ui, work, type GlobalView } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import { Button, currentPlatform, Icon, IconButton, Kbd, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { rowKey } from '../views/inbox/groups';
  import { currentSections, goToRow } from '../views/inbox/now';
  import { workTitle } from '../views/work/live';
  import { workKey } from '../views/work/phase';
  import { backTarget, type JumpLoc } from './jumplist';
  import { chordFor, currentRun, runPhase } from './labels';
  import {
    activateProject,
    currentTab,
    focusedPane,
    focusedSession,
    isToolTab,
    openRuns,
    rowLabel,
    selectRow,
    toolSubLines,
    VIEW_NAMES,
  } from './nav';
  import ScopeSelect from './ScopeSelect.svelte';

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
    const tb = layout.get(loc.project)?.tabs.find((x) => x.id === loc.tab);
    return `${projects.byId(loc.project)?.name ?? loc.project} › ${tb ? rowLabel(tb) : ''}`;
  }

  /** The breadcrumb as text: the window title in native mode. */
  const title = $derived.by(() => {
    if (view) return VIEW_NAMES[view];
    if (!project) return 'Kelta';
    if (tool) return `${t('Tools')} › ${tool.label}`;
    if (item) return `${project.name} › ${workKey(item)} ${workTitle(item)}`;
    return row ? `${project.name} › ${row.text}` : project.name;
  });

  $effect(() => {
    const text = title;
    document.title = text;
    // Outside a Tauri window getCurrentWindow() throws; the catch swallows it.
    if (custom) return;
    import('@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => getCurrentWindow().setTitle(text))
      .catch(() => {});
  });

  // ---- the focused session's Claude workflow run (#272) -----------------------------------------

  const run = $derived(currentRun(session ? sessions.runs[session.id] : undefined));
  /** `<workflow> · <phase with a running agent>`, or its final status. */
  const runChip = $derived(run ? `${run.name ?? t('workflow')} · ${runPhase(run)}` : null);
  $effect(() => {
    if (session?.kind.type === 'claude') void sessions.loadRuns(session.id);
  });

  // ---- actions ---------------------------------------------------------------------------------

  /** A tool's context chip: the tab of its work item, else the project's first row. */
  function goOwner(): void {
    if (!project) return;
    const tabs = layout.get(project.id)?.tabs ?? [];
    const tb =
      (ownerItem && tabs.find((x) => x.work_item_id === ownerItem.id)) ??
      tabs.find((x) => !isToolTab(x, lines));
    if (tb) selectRow(project.id, tb.id);
  }

  // ---- Claude sessions waiting on me, wherever they run (#372): Now's "Claude needs you" ----------

  const asking = $derived(currentSections().find((s) => s.id === 'needs_you')?.rows ?? []);
  let askMenu = $state<{ x: number; y: number } | null>(null);
  const ASKING: MenuItem[] = $derived(
    asking.map((r) =>
      r.type === 'work'
        ? { id: r.id, label: workTitle(r.item), detail: r.phase.label }
        : { id: r.id, label: rowKey(r) },
    ),
  );

  function onAsking(e: MouseEvent): void {
    // One waiting: straight to it (attention.next would cycle past it once it was the last jump).
    if (asking.length === 1) void goToRow(asking[0]!);
    else {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      askMenu = { x: r.left, y: r.bottom + 2 };
    }
  }

  const searchChord = $derived(chordFor('palette.open'));

  let plusMenu = $state<{ x: number; y: number } | null>(null);
  const PLUS: MenuItem[] = $derived([
    { id: 'work.new', label: t('New work item'), icon: 'git-branch', kbd: chordFor('work.new') },
    { id: 'session.new', label: t('New session…'), icon: 'square-terminal', kbd: chordFor('session.new') },
  ]);

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
    label={back ? t('Back to {place}', { place: placeLabel(back) }) : t('Back')}
    chord={chordFor('nav.back')}
    data-testid="context-back"
    onclick={() => void dispatch('nav.back')}
  />
  <IconButton
    icon="chevron-right"
    size="sm"
    label={t('Go forward')}
    chord={chordFor('nav.forward')}
    data-testid="context-forward"
    onclick={() => void dispatch('nav.forward')}
  />

  {#if !item}
  <span class="crumb" data-tauri-drag-region data-testid="context-crumb">
    {#if view}
      <span class="here" data-tauri-drag-region
        ><Icon name={VIEW_ICONS[view]} size={14} /> {VIEW_NAMES[view]}</span
      >
    {:else if tool}
      <span class="parent" data-tauri-drag-region>{t('Tools')}</span>
      <span class="sep" aria-hidden="true">›</span>
      <span class="here" data-tauri-drag-region><span aria-hidden="true">⚒</span>{tool.label}</span>
    {:else if project}
      <span class="project" style:--project-color={projectColor} data-tauri-drag-region>{project.name}</span>
      {#if row}
        <span class="sep" aria-hidden="true">›</span>
        <span class="here" data-tauri-drag-region
          >{#if row.glyph}<span aria-hidden="true">{row.glyph}</span>{/if}{row.text}</span
        >
      {/if}
    {/if}
  </span>
  {/if}

  {#if tool && project}
    {#if isHome}
      <span class="muted" data-tauri-drag-region>{t('for you')}</span>
    {:else}
      <Button
        size="sm"
        variant="ghost"
        class="chip owner"
        title={t('Go to {name}', { name: project.name })}
        data-testid="context-owner"
        onclick={goOwner}
        ><span class="project" style:--project-color={projectColor}>{project.name}</span>{#if ownerItem}<span
            class="sep">›</span
          ><span class="key">{workKey(ownerItem)}</span>{/if}</Button
      >
      {#if cwd}<span class="meta" title={session?.cwd} data-tauri-drag-region>{cwd}</span>{/if}
    {/if}
  {/if}
  <span class="spacer" data-tauri-drag-region></span>
  <button
    type="button"
    class="search"
    title={t('Search or run a command')}
    data-testid="context-search"
    onclick={() => void dispatch('palette.open')}
  >
    <Icon name="search" size={14} /><span class="search-text">{t('Search or run…')}</span>
    {#if searchChord}<span aria-hidden="true"><Kbd chord={searchChord} /></span>{/if}
  </button>
  <span class="spacer" data-tauri-drag-region></span>
  {#if view && project}
    <Button
      size="sm"
      variant="ghost"
      class="chip"
      chord={chordFor('nav.back')}
      title={t('Leave {view} for what is underneath', { view: VIEW_NAMES[view] })}
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
        title={t('Back to {place}', { place: placeLabel(back) })}
        data-testid="context-return"
        onclick={() => void dispatch('nav.back')}
        >↩ {placeLabel(back)}{#if tool.keybinding}
          <Kbd chord={tool.keybinding} />{/if}</Button
      >
    {/if}
  {:else if cwd && !item}
    <span class="meta" title={session?.cwd} data-testid="context-meta" data-tauri-drag-region
      >{cwd}{#if model}<span class="sep">·</span>{model}{/if}</span
    >
  {/if}

  {#if runChip && session}
    {@const sid = session.id}
    <Button
      size="sm"
      variant="ghost"
      class="chip"
      title={t('Workflow runs of this session')}
      data-testid="context-workflow"
      onclick={() => void openRuns(sid)}><span aria-hidden="true">⚙</span> {runChip}</Button
    >
  {/if}

  <ScopeSelect />

  {#if asking.length > 0}
    <Button
      size="sm"
      variant="ghost"
      class="chip"
      chord={chordFor('attention.next')}
      title={t('Claude needs you')}
      aria-haspopup={asking.length > 1 ? 'menu' : undefined}
      data-testid="context-asking"
      onclick={onAsking}
      ><Lamp level="needs_input" label={tn('{n} needs you', '{n} need you', asking.length)} /></Button
    >
  {/if}

  <Button
    variant="ghost"
    size="sm"
    icon="plus"
    class="plus"
    aria-haspopup="menu"
    data-testid="tab-plus"
    onclick={(e) => {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      plusMenu = { x: r.right - 220, y: r.bottom + 2 };
    }}>{t('New')}</Button
  >

  {#if custom && !mac}
    <span class="controls">
      <IconButton icon="minus" label={t('Minimize')} size="sm" onclick={() => void win('minimize')} />
      <IconButton icon="square" label={t('Maximize')} size="sm" onclick={() => void win('toggleMaximize')} />
      <IconButton icon="x" label={t('Close window')} size="sm" onclick={() => void win('close')} />
    </span>
  {/if}
</header>

{#if custom && !mac}
  {#each HANDLES as dir (dir)}
    <div class="handle {dir}" aria-hidden="true" onpointerdown={(e) => void resize(e, dir)}></div>
  {/each}
{/if}

{#if askMenu}
  <Menu
    items={ASKING}
    x={askMenu.x}
    y={askMenu.y}
    label={t('Claude needs you')}
    onselect={(id) => {
      const row = asking.find((r) => r.id === id);
      if (row) void goToRow(row);
    }}
    onclose={() => (askMenu = null)}
  />
{/if}

{#if plusMenu}
  <Menu
    items={PLUS}
    x={plusMenu.x}
    y={plusMenu.y}
    label={t('New')}
    onselect={(id) =>
      void dispatch(id).catch((err: unknown) => toasts.error(err, t('Could not open the view')))}
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

  .here {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
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

  /* The search field (#429): it opens the palette, the rail has no search row. */
  .search {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    flex: 0 1 320px;
    min-width: 0;
    height: var(--k-control-height-sm);
    padding: 0 var(--k-space-2) 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
    color: var(--k-fg-muted);
    font: inherit;
    cursor: pointer;
    text-align: left;
  }

  .search:hover {
    border-color: var(--k-border-strong);
    color: var(--k-fg);
  }

  .search:focus-visible {
    outline: 2px solid var(--k-focus);
    outline-offset: -2px;
  }

  .search-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .controls {
    display: inline-flex;
    flex: none;
    gap: var(--k-space-1);
  }

  /* Narrow bars (#248): the cwd and model go first, then the search text (its icon stays). */
  @container contextbar (max-width: 1280px) {
    .meta {
      display: none;
    }
  }

  @container contextbar (max-width: 840px) {
    .search-text,
    .search :global(.k-kbd) {
      display: none;
    }

    .search {
      flex: none;
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
