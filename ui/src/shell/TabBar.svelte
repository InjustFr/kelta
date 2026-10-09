<script lang="ts">
  import { dispatch } from '$lib/actions';
  import type { ProjectId, Tab, ToolInfo } from '$lib/gen';
  import { activeTab, allPanes, moveTab, paneSession } from '$lib/layout';
  import { layout, toasts, tools, ui } from '$lib/stores';
  import { Icon, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { requestCloseTab, selectTab, tabAttention } from './nav';

  interface Props {
    projectId: ProjectId;
  }

  let { projectId }: Props = $props();

  const current = $derived(layout.get(projectId));
  const tabs = $derived(current?.tabs ?? []);
  const active = $derived(current ? activeTab(current) : null);
  const toolList = $derived(tools.list(projectId));

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
      { id: 'session', label: 'New session…', icon: 'square-terminal' },
      { id: 'tickets', label: 'Tickets', icon: 'ticket', separator: true },
      { id: 'reviews', label: 'Reviews', icon: 'git-pull-request' },
    ];
    toolList.slice(0, 12).forEach((t: ToolInfo, i) => {
      items.push({
        id: `tool:${t.id}`,
        label: t.label,
        icon: t.icon ?? 'wrench',
        disabled: t.installed === false,
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
      <span class="lamp-slot"><Lamp {level} /></span>
      {#if tab.work_item_id}<Icon name="git-branch" size={12} />{/if}
      <span class="title" class:session={tab.work_item_id || allPanes(tab.root).some((p) => paneSession(p))}
        >{tab.title}</span
      >
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
        <Icon name="x" size={12} />
      </button>
    </div>
  {/each}
  <button
    type="button"
    class="plus"
    aria-label="New tab"
    title="New tab"
    data-testid="tab-plus"
    onclick={(e) => {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      plusMenu = { x: r.left, y: r.bottom + 2 };
    }}
  >
    <Icon name="plus" size={15} />
  </button>
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
    align-items: stretch;
    height: var(--k-tabbar-height);
    flex: none;
    padding: 0 var(--k-space-2);
    background: var(--k-bezel-raised);
    font-size: var(--k-font-size-sm);
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    max-width: 220px;
    padding: 0 var(--k-space-2) 0 var(--k-space-3);
    box-shadow: inset 0 -2px 0 transparent;
    color: var(--k-fg-chrome);
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--k-duration) ease-out;
  }

  .tab:hover {
    color: var(--k-fg);
  }

  .tab:focus-visible {
    outline-offset: -2px;
  }

  .tab.active {
    box-shadow: inset 0 -2px 0 var(--k-accent);
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
  }

  /* Claude waiting: a top edge that stays visible even when the tab is half scrolled out. */
  .tab.asks {
    box-shadow: inset 0 2px 0 var(--k-lamp-needs-input);
  }

  .tab.asks.active {
    box-shadow:
      inset 0 2px 0 var(--k-lamp-needs-input),
      inset 0 -2px 0 var(--k-accent);
  }

  .tab.over {
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  /* Fixed width even when empty, so the states of all tabs line up in one column. */
  .lamp-slot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 10px;
    flex: none;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Session tabs (session / branch) in mono, view tabs in the UI face (DESIGN §6.3). */
  .title.session {
    font-family: var(--k-font-mono);
  }

  .close {
    display: inline-flex;
    padding: 2px;
    border: none;
    border-radius: var(--k-radius-sm);
    background: transparent;
    color: inherit;
    visibility: hidden;
    cursor: pointer;
  }

  .tab:hover .close,
  .tab:focus-visible .close,
  .tab.active .close {
    visibility: visible;
  }

  .close:hover {
    background: var(--k-bg-active);
  }

  .plus {
    align-self: center;
    display: inline-flex;
    padding: 4px;
    margin-left: var(--k-space-2);
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
  }

  .plus:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }
</style>
