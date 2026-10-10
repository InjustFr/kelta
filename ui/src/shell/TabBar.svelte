<script lang="ts">
  import { dispatch } from '$lib/actions';
  import type { ProjectId, Tab, ToolInfo } from '$lib/gen';
  import { activeTab, moveTab } from '$lib/layout';
  import { layout, projects, toasts, tools, ui } from '$lib/stores';
  import { Button, Icon, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { chordFor } from './labels';
  import { requestCloseTab, selectTab, tabAttention } from './nav';

  interface Props {
    projectId: ProjectId;
  }

  let { projectId }: Props = $props();

  const current = $derived(layout.get(projectId));
  const tabs = $derived(current?.tabs ?? []);
  const active = $derived(current ? activeTab(current) : null);
  const toolList = $derived(tools.list(projectId));
  const projectName = $derived(projects.byId(projectId)?.name ?? '');

  let tabMenu = $state<{ tab: Tab; x: number; y: number } | null>(null);
  let plusMenu = $state<{ x: number; y: number } | null>(null);
  let dragId = $state<string | null>(null);
  let overId = $state<string | null>(null);

  $effect(() => {
    // Tools feed the "+" menu; they are cached per project.
    if (tools.byProject[projectId] === undefined) void tools.load(projectId);
  });

  function tabMenuItems(tab: Tab): MenuItem[] {
    const index = tabs.findIndex((t) => t.id === tab.id);
    return [
      { id: 'close', label: 'Close tab', icon: 'x' },
      { id: 'left', label: 'Move left', icon: 'arrow-left', disabled: index <= 0, separator: true },
      { id: 'right', label: 'Move right', icon: 'arrow-right', disabled: index >= tabs.length - 1 },
    ];
  }

  function onTabMenu(tab: Tab, id: string): void {
    const index = tabs.findIndex((t) => t.id === tab.id);
    if (id === 'close') void requestCloseTab(projectId, tab.id);
    else if (id === 'left') layout.update(projectId, (l) => moveTab(l, tab.id, index - 1));
    else if (id === 'right') layout.update(projectId, (l) => moveTab(l, tab.id, index + 1));
  }

  function plusItems(): MenuItem[] {
    const items: MenuItem[] = [
      { id: 'session', label: 'New session…', icon: 'square-terminal', kbd: chordFor('session.new') },
      { id: 'tickets', label: 'Tickets', icon: 'ticket', kbd: chordFor('tickets.open') },
      { id: 'reviews', label: 'Reviews', icon: 'git-pull-request', kbd: chordFor('reviews.open') },
    ];
    toolList.slice(0, 12).forEach((t: ToolInfo, i) => {
      items.push({
        id: `tool:${t.id}`,
        label: t.label,
        icon: t.icon ?? 'wrench',
        kbd: t.keybinding ?? undefined,
        disabled: t.installed === false,
        title: t.installed === false ? 'Not installed' : undefined,
        separator: i === 0,
      });
    });
    return items;
  }

  async function onPlus(id: string): Promise<void> {
    try {
      if (id === 'session') ui.openSheet('session_new');
      else if (id === 'tickets') await dispatch('tickets.open');
      else if (id === 'reviews') await dispatch('reviews.open');
      else if (id.startsWith('tool:')) await dispatch('tools.open', { tool_id: id.slice(5) });
    } catch (err) {
      toasts.error(err, 'Could not open the view');
    }
  }

  function onDrop(targetId: string): void {
    const from = dragId;
    dragId = null;
    overId = null;
    if (!from || from === targetId) return;
    const to = tabs.findIndex((t) => t.id === targetId);
    if (to >= 0) layout.update(projectId, (l) => moveTab(l, from, to));
  }

  function onAux(e: MouseEvent, tab: Tab): void {
    if (e.button === 1) {
      e.preventDefault();
      void requestCloseTab(projectId, tab.id);
    }
  }
</script>

<div class="tabbar" role="tablist" aria-label="Tabs" data-testid="tabbar">
  {#if projectName}<span class="project-name">{projectName}</span>{/if}
  {#each tabs as tab (tab.id)}
    {@const level = tabAttention(tab)}
    <div
      class="tab"
      class:active={active?.id === tab.id}
      class:asks={level === 'needs_input'}
      class:over={overId === tab.id && dragId !== tab.id}
      role="tab"
      tabindex="0"
      aria-selected={active?.id === tab.id}
      data-testid="tab"
      data-tab-id={tab.id}
      draggable="true"
      onclick={() => selectTab(projectId, tab.id)}
      onkeydown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          selectTab(projectId, tab.id);
        } else if (e.key === 'Delete' || e.key === 'Backspace') {
          e.preventDefault();
          void requestCloseTab(projectId, tab.id);
        }
      }}
      onauxclick={(e) => onAux(e, tab)}
      onmousedown={(e) => {
        if (e.button === 1) e.preventDefault(); // no autoscroll on middle click
      }}
      oncontextmenu={(e) => {
        e.preventDefault();
        tabMenu = { tab, x: e.clientX, y: e.clientY };
      }}
      ondragstart={(e) => {
        dragId = tab.id;
        e.dataTransfer?.setData('text/plain', tab.id);
      }}
      ondragover={(e) => {
        if (!dragId) return;
        e.preventDefault();
        overId = tab.id;
      }}
      ondragleave={() => (overId = null)}
      ondrop={(e) => {
        e.preventDefault();
        onDrop(tab.id);
      }}
      ondragend={() => {
        dragId = null;
        overId = null;
      }}
    >
      {#if level !== 'none'}<span class="lamp-slot"><Lamp {level} /></span>{/if}
      {#if tab.work_item_id}<Icon name="git-branch" size={14} />{/if}
      <span class="title">{tab.title}</span>
      <button
        type="button"
        class="close"
        aria-label="Close tab {tab.title}"
        tabindex="-1"
        onclick={(e) => {
          e.stopPropagation();
          void requestCloseTab(projectId, tab.id);
        }}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  {/each}
  <Button
    variant="ghost"
    size="sm"
    icon="plus"
    class="plus"
    data-testid="tab-plus"
    onclick={(e) => {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      plusMenu = { x: r.left, y: r.bottom + 2 };
    }}>New tab</Button
  >
</div>

{#if tabMenu}
  {@const target = tabMenu.tab}
  <Menu
    items={tabMenuItems(target)}
    x={tabMenu.x}
    y={tabMenu.y}
    label="Tab actions"
    onselect={(id) => onTabMenu(target, id)}
    onclose={() => (tabMenu = null)}
  />
{/if}
{#if plusMenu}
  <Menu
    items={plusItems()}
    x={plusMenu.x}
    y={plusMenu.y}
    label="Open"
    onselect={(id) => void onPlus(id)}
    onclose={() => (plusMenu = null)}
  />
{/if}

<style>
  .tabbar {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    height: var(--k-tabbar-height);
    flex: none;
    padding: 0 var(--k-space-3);
    background: var(--k-bezel);
    font-size: var(--k-font-size-sm);
    overflow-x: auto;
    /* The thin bar is the overflow indicator. */
    scrollbar-width: thin;
  }

  /* Only when the sidebar is compact and no longer names the project. */
  .project-name {
    display: none;
    flex: none;
    padding-right: var(--k-space-4);
    margin-right: var(--k-space-2);
    border-right: 1px solid var(--k-border);
    font-size: var(--k-font-size-lg);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg);
    white-space: nowrap;
  }

  @media (max-width: 1099px) {
    .project-name {
      display: inline-block;
    }
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    flex: none;
    height: var(--k-control-height);
    max-width: 240px;
    padding: 0 6px 0 var(--k-space-4);
    border-radius: var(--k-radius);
    color: var(--k-fg-chrome);
    font-weight: var(--k-weight-medium);
    white-space: nowrap;
    cursor: pointer;
    transition:
      background var(--k-duration) ease-out,
      color var(--k-duration) ease-out;
  }

  .tab:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .tab:focus-visible {
    outline-offset: -2px;
  }

  /* Neighbouring plain tabs get a hairline between them so each one reads as its own target. */
  .tab {
    position: relative;
  }

  .tab:not(.active, .asks, :hover) + .tab:not(.active, .asks, :hover)::before {
    content: '';
    position: absolute;
    left: calc(-1 * var(--k-space-2) / 2 - 0.5px);
    top: 25%;
    bottom: 25%;
    width: 1px;
    background: var(--k-border);
  }

  /* Call light (§4.4): only while not selected; the lamp carries it on the active tab. */
  .tab.asks {
    --k-fg-subtle: var(--k-fg-muted);
    background: var(--k-lit);
    color: var(--k-fg);
  }

  .tab.active {
    background: var(--k-well);
    box-shadow: 0 0 0 1px var(--k-border);
    color: var(--k-fg);
  }

  .tab.over {
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  .lamp-slot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 12px;
    flex: none;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    position: relative;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  /* The visible box stays small; the target reaches the 28px floor. */
  .close::before {
    content: '';
    position: absolute;
    inset: -2px;
  }

  /* Plain tabs show the close button only on hover or focus, but its slot is always reserved so the
     tab never changes width under the pointer; the active tab always shows it. */
  .tab:not(.active) .close {
    visibility: hidden;
  }

  .tab:not(.active):is(:hover, :focus-within) .close {
    visibility: visible;
  }

  .close:hover {
    background: var(--k-bg-active);
  }

  .tabbar :global(.plus) {
    flex: none;
    color: var(--k-fg-chrome);
  }
</style>
