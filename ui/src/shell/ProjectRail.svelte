<script lang="ts">
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import type { ProjectInfo } from '$lib/gen';
  import { attention, projects, sessions, toasts, ui } from '$lib/stores';
  import { Icon, isIconName, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { nowSummary, refreshNow } from '../views/inbox/now';
  import { confirms } from './confirm.svelte';
  import { activateProject, openInbox, projectAttention, railProjects } from './nav';

  const rail = $derived(railProjects());
  const reorderable = $derived(rail.filter((p) => !p.builtin));
  const home = $derived(rail.find((p) => p.builtin) ?? null);

  // Now's tile: badge = what waits on Louis (first four sections), tooltip = the split header.
  const now = $derived(nowSummary());

  // Startup and window focus refresh Now's sources (no polling).
  onMount(() => void refreshNow());

  let menu = $state<{ project: ProjectInfo; x: number; y: number } | null>(null);
  let dragId = $state<string | null>(null);
  let overId = $state<string | null>(null);

  function glyph(p: ProjectInfo): string {
    const icon = p.icon?.trim();
    if (icon && !isIconName(icon)) return [...icon].slice(0, 2).join('');
    return (p.name.trim()[0] ?? '?').toUpperCase();
  }

  function iconName(p: ProjectInfo): string | null {
    if (p.builtin) return 'house';
    const icon = p.icon?.trim();
    return icon && isIconName(icon) ? icon : null;
  }

  function dotTitle(p: ProjectInfo): string {
    const lvl = projectAttention(p.id);
    const n = attention.forProject(p.id).needs_input_count;
    return lvl === 'needs_input' && n > 0
      ? `${n} session${n === 1 ? '' : 's'} need input`
      : lvl.replace('_', ' ');
  }

  function menuItems(p: ProjectInfo): MenuItem[] {
    const index = reorderable.findIndex((x) => x.id === p.id);
    const items: MenuItem[] = [{ id: 'settings', label: 'Project settings…', icon: 'settings' }];
    if (!p.builtin) {
      items.push(
        { id: 'up', label: 'Move up', icon: 'arrow-up', disabled: index <= 0, separator: true },
        { id: 'down', label: 'Move down', icon: 'arrow-down', disabled: index >= reorderable.length - 1 },
        { id: 'close', label: 'Close project', icon: 'x', separator: true },
        { id: 'close_kill', label: 'Close and stop sessions', icon: 'square', danger: true },
        { id: 'remove', label: 'Remove project…', icon: 'trash-2', danger: true },
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
          await move(p.id, id === 'up' ? -1 : 1);
          break;
        case 'close':
          await closeProject(p, false);
          break;
        case 'close_kill': {
          const n = sessions.forProject(p.id).filter((s) => s.lifecycle === 'live').length;
          const answer = await confirms.ask({
            title: `Close ${p.name} and stop its sessions?`,
            body: `${n} running session${n === 1 ? '' : 's'} will be terminated.`,
            tone: 'danger',
            actions: [{ id: 'stop', label: 'Close and stop sessions', variant: 'danger' }],
          });
          if (answer === 'stop') await closeProject(p, true);
          break;
        }
        case 'remove': {
          const answer = await confirms.ask({
            title: `Remove ${p.name}?`,
            body: 'The project configuration is moved to projects/.trash. Running sessions keep running.',
            tone: 'danger',
            actions: [{ id: 'remove', label: 'Remove project', variant: 'danger' }],
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
      toasts.error(err, 'Project action failed');
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

  async function move(id: string, delta: -1 | 1): Promise<void> {
    const ids = reorderable.map((p) => p.id);
    const at = ids.indexOf(id);
    const to = at + delta;
    if (at < 0 || to < 0 || to >= ids.length) return;
    ids.splice(to, 0, ...ids.splice(at, 1));
    await applyOrder(ids);
  }

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
    const ids = reorderable.map((p) => p.id);
    const fromAt = ids.indexOf(from);
    const toAt = ids.indexOf(targetId);
    if (fromAt < 0 || toAt < 0) return;
    ids.splice(toAt, 0, ...ids.splice(fromAt, 1));
    try {
      await applyOrder(ids);
    } catch (err) {
      toasts.error(err, 'Reordering projects failed');
    }
  }

  function openMenu(e: MouseEvent, p: ProjectInfo): void {
    e.preventDefault();
    menu = { project: p, x: e.clientX, y: e.clientY };
  }
</script>

<svelte:window onfocus={() => void refreshNow()} />

<nav class="rail" aria-label="Projects" data-testid="rail">
  <button
    type="button"
    class="item inbox"
    class:active={ui.inboxActive}
    onclick={openInbox}
    title={`Now: ${now.header}`}
    aria-label="Now"
    aria-current={ui.inboxActive ? 'page' : undefined}
    data-testid="rail-inbox"
  >
    <Icon name="inbox" size={16} />
    {#if now.waiting > 0}
      <span class="badge" data-testid="inbox-badge">{now.waiting > 99 ? '99+' : now.waiting}</span>
    {/if}
  </button>

  <div class="list" role="list">
    {#each reorderable as p (p.id)}
      {@const lvl = projectAttention(p.id)}
      {@const n = attention.forProject(p.id).needs_input_count}
      <div role="listitem" class="slot" class:over={overId === p.id && dragId !== p.id}>
        <button
          type="button"
          class="item project"
          class:active={p.active && !ui.inboxActive}
          class:lit={lvl === 'needs_input'}
          style:--project-color={p.color ?? 'var(--k-border-strong)'}
          title={p.name}
          aria-label={p.name}
          aria-current={p.active && !ui.inboxActive ? 'page' : undefined}
          data-testid="rail-project"
          data-project-id={p.id}
          data-attention={lvl}
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
        >
          {#if iconName(p)}<Icon name={iconName(p) ?? ''} size={14} />{:else}<span class="glyph"
              >{glyph(p)}</span
            >{/if}
          {#if lvl !== 'none'}
            <span class="att">
              <Lamp level={lvl} title={dotTitle(p)} />
              {#if lvl === 'needs_input' && n > 1}<span class="count k-num">{n}</span>{/if}
            </span>
          {/if}
        </button>
      </div>
    {/each}
  </div>

  {#if home}
    {@const lvl = projectAttention(home.id)}
    <button
      type="button"
      class="item project home"
      class:active={home.active && !ui.inboxActive}
      title="Home"
      aria-label="Home"
      aria-current={home.active && !ui.inboxActive ? 'page' : undefined}
      data-testid="rail-project"
      data-project-id={home.id}
      data-attention={lvl}
      onclick={() => activateProject(home.id)}
      oncontextmenu={(e) => openMenu(e, home)}
    >
      <Icon name="house" size={16} />
      {#if lvl !== 'none'}<span class="att"><Lamp level={lvl} title={dotTitle(home)} /></span>{/if}
    </button>
  {/if}
  <button
    type="button"
    class="item"
    title="Settings"
    aria-label="Settings"
    onclick={() => void dispatch('settings.open')}
    data-testid="rail-settings"
  >
    <Icon name="settings" size={16} />
  </button>
  <button
    type="button"
    class="item add"
    title="Open or create a project"
    aria-label="New project"
    onclick={() => ui.openSheet('project_new')}
    data-testid="rail-add"
  >
    <Icon name="plus" size={16} />
  </button>
</nav>

{#if menu}
  {@const target = menu.project}
  <Menu
    items={menuItems(target)}
    x={menu.x}
    y={menu.y}
    label="Project actions"
    onselect={(id) => void onMenu(target, id)}
    onclose={() => (menu = null)}
  />
{/if}

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--k-space-2);
    width: var(--k-rail-width);
    flex: none;
    padding: var(--k-space-3) 0;
    background: var(--k-bezel);
  }

  .list {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--k-space-3);
    flex: 1;
    min-height: 0;
    width: 100%;
    padding: var(--k-space-3) 0;
    overflow-y: auto;
    scrollbar-width: none;
  }

  .slot {
    border-radius: var(--k-radius);
  }

  .slot.over {
    box-shadow: 0 -4px 0 -2px var(--k-accent);
  }

  .item {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
    transition: background var(--k-duration) ease-out;
  }

  .item:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .item:focus-visible {
    outline: none;
    box-shadow:
      0 0 0 2px var(--k-bezel),
      0 0 0 4px var(--k-focus);
  }

  /* Only the current destination (a project, home or the inbox) wears a tile face. */
  .item.active,
  .project:not(.home) {
    background: var(--k-well);
    overflow: visible;
  }

  /* Index bar: raw project colour, short when idle, full height on the current destination. */
  .project:not(.home)::before,
  .item.active::before {
    content: '';
    position: absolute;
    left: 0;
    top: 8px;
    bottom: 8px;
    width: 3px;
    border-radius: var(--k-radius) 0 0 var(--k-radius);
    background: var(--project-color, var(--k-fg-muted));
  }

  .item.active::before {
    top: 0;
    bottom: 0;
  }

  .item.active,
  .project:not(.home):hover {
    color: var(--k-fg);
  }

  .project.active .glyph {
    font-weight: var(--k-weight-strong);
  }

  /* Claude waiting is the brightest thing in the window. */
  .project.lit {
    background: color-mix(in oklab, var(--k-lamp-needs-input) var(--k-lit-mix), var(--k-well));
  }

  .glyph {
    padding-left: 2px;
    font-size: var(--k-font-size);
  }

  .att {
    position: absolute;
    top: -4px;
    right: -5px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    min-width: 10px;
    height: 10px;
  }

  .count {
    font-size: 10px;
    font-weight: var(--k-weight-strong);
    line-height: 1;
    color: var(--k-lamp-needs-input);
  }

  .badge {
    position: absolute;
    top: -4px;
    right: -6px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--k-lamp-needs-input);
    color: var(--k-well);
    font-size: 10px;
    font-weight: var(--k-weight-strong);
    font-variant-numeric: tabular-nums;
    line-height: 14px;
    text-align: center;
  }
</style>
