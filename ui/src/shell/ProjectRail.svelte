<script lang="ts">
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import type { ProjectInfo } from '$lib/gen';
  import { attention, projects, sessions, toasts, ui } from '$lib/stores';
  import { Button, Icon, IconButton, isIconName, Kbd, Lamp, Menu, type MenuItem } from '$lib/ui';

  import { readyByProject } from '../views/inbox/groups';
  import { nowSummary, refreshNow } from '../views/inbox/now';
  import { confirms } from './confirm.svelte';
  import { attentionLabel, chordFor } from './labels';
  import { activateProject, openInbox, projectAttention, railProjects } from './nav';

  const rail = $derived(railProjects());
  const reorderable = $derived(rail.filter((p) => !p.builtin));
  const home = $derived(rail.find((p) => p.builtin) ?? null);

  // Now's tile: badge = what waits on Louis (first four sections), tooltip = the split header.
  const now = $derived(nowSummary());
  const ready = $derived(readyByProject(now.sections));

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

  function subLine(p: ProjectInfo): string {
    const lvl = projectAttention(p.id);
    const n = attention.forProject(p.id).needs_input_count;
    if (lvl === 'needs_input' && n > 1) return `${n} need input`;
    return attentionLabel(lvl) || (p.builtin ? 'Terminals in your home folder' : '');
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

{#snippet chord(id: string)}
  {@const c = chordFor(id)}
  {#if c}<span class="kbd" aria-hidden="true"><Kbd chord={c} /></span>{/if}
{/snippet}

{#snippet face(p: ProjectInfo)}
  {@const lvl = projectAttention(p.id)}
  {@const sub = subLine(p)}
  <span class="tile" aria-hidden="true">
    {#if iconName(p)}<Icon name={iconName(p) ?? ''} size={16} />{:else}<span class="glyph">{glyph(p)}</span
      >{/if}
  </span>
  <span class="text">
    <span class="name">{p.name}</span>
    {#if sub}<span class="sub">{sub}</span>{/if}
  </span>
  {#if ready.get(p.id)}
    <span class="ready k-num" title="Ready for review" data-testid="rail-ready">{ready.get(p.id)}</span>
  {/if}
  <span class="lamp-slot" aria-hidden="true"><Lamp level={lvl} /></span>
{/snippet}

{#snippet more(p: ProjectInfo)}
  <IconButton
    class="more"
    size="sm"
    icon="ellipsis"
    label={`Menu for ${p.name}`}
    onclick={(e) => openMenu(e, p)}
  />
{/snippet}

<nav class="rail" aria-label="Projects" data-testid="rail">
  <button
    type="button"
    class="search"
    title="Search or run a command"
    onclick={() => void dispatch('palette.open')}
    data-testid="rail-search"
  >
    <Icon name="search" size={16} />
    <span class="search-text">Search or run…</span>
    {@render chord('palette.open')}
  </button>

  <button
    type="button"
    class="item inbox"
    class:active={ui.inboxActive}
    onclick={openInbox}
    title={`Now: ${now.header}`}
    aria-current={ui.inboxActive ? 'page' : undefined}
    data-testid="rail-inbox"
  >
    <span class="tile plain"><Icon name="inbox" size={18} /></span>
    <span class="text">
      <span class="name">Now</span>
      <span class="sub">{now.header || 'Nothing waiting'}</span>
    </span>
    {#if now.waiting > 0}
      <span class="badge k-num" data-testid="inbox-badge">{now.waiting > 99 ? '99+' : now.waiting}</span>
    {/if}
  </button>

  <div class="group">
    <div class="heading">
      <h2>Projects</h2>
      <Button
        variant="ghost"
        size="sm"
        icon="plus"
        title="Open or create a project"
        onclick={() => ui.openSheet('project_new')}
        data-testid="rail-add">Add project</Button
      >
    </div>

    <ul class="projects">
      {#each reorderable as p (p.id)}
        {@const lvl = projectAttention(p.id)}
        {@const active = p.active && !ui.inboxActive}
        <li
          class="row"
          class:active
          class:over={overId === p.id && dragId !== p.id}
          style:--project-color={p.color ?? 'var(--k-border-strong)'}
        >
          <button
            type="button"
            class="item project"
            class:active
            class:lit={lvl === 'needs_input'}
            title={p.name}
            aria-current={active ? 'page' : undefined}
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
            {@render face(p)}
          </button>
          {@render more(p)}
        </li>
      {:else}
        <li class="none">No projects yet</li>
      {/each}
    </ul>
  </div>

  <div class="footer">
    {#if home}
      {@const lvl = projectAttention(home.id)}
      {@const active = home.active && !ui.inboxActive}
      <div class="row" class:active style:--project-color={home.color ?? 'var(--k-border-strong)'}>
        <button
          type="button"
          class="item project home"
          class:active
          class:lit={lvl === 'needs_input'}
          title={home.name}
          aria-current={active ? 'page' : undefined}
          data-testid="rail-project"
          data-project-id={home.id}
          data-attention={lvl}
          onclick={() => activateProject(home.id)}
          oncontextmenu={(e) => openMenu(e, home)}
        >
          {@render face(home)}
        </button>
        {@render more(home)}
      </div>
    {/if}
    <button
      type="button"
      class="item settings"
      title="Settings"
      onclick={() => void dispatch('settings.open')}
      data-testid="rail-settings"
    >
      <span class="tile plain"><Icon name="settings" size={18} /></span>
      <span class="text"><span class="name">Settings</span></span>
      {@render chord('settings.open')}
    </button>
  </div>
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
    flex: none;
  }

  /* Rows: tile | name over status word | lamp or "⋯". */
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
  .search:focus-visible {
    outline: 2px solid var(--k-focus);
    outline-offset: -2px;
  }

  .item.active {
    background-color: var(--k-bg-selected);
  }

  .inbox.active {
    box-shadow: inset 3px 0 0 var(--k-accent);
  }

  .project.active {
    box-shadow: inset 3px 0 0 var(--project-color);
  }

  .project.active .name {
    font-weight: var(--k-weight-strong);
  }

  /* Call light: a session waits on you here. Selected rows keep their face; the lamp and word speak. */
  .project.lit:not(.active) {
    --k-fg-subtle: var(--k-fg-muted); /* only fg and fg-muted read on lit (§4.4) */
    background-color: var(--k-lit);
  }

  .tile {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: var(--k-radius);
    background: color-mix(in oklab, var(--project-color) 28%, var(--k-well));
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--project-color) 55%, transparent);
    color: var(--k-fg);
  }

  .tile.plain {
    background: none;
    box-shadow: none;
    color: var(--k-fg-chrome);
  }

  .glyph {
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-strong);
    line-height: 1;
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

  .ready {
    flex: none;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--k-bg-selected);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    line-height: 18px;
    text-align: center;
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

  .projects {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    scrollbar-width: thin;
  }

  .row {
    position: relative;
    flex: none;
    border-radius: var(--k-radius);
  }

  .row.over {
    box-shadow: 0 -2px 0 0 var(--k-accent);
  }

  .lamp-slot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 28px;
  }

  /* "⋯" shares the lamp's slot: it shows on hover, keyboard focus and the current row. */
  .row :global(.more) {
    position: absolute;
    top: calc((var(--k-sidebar-row-height) - var(--k-control-height-sm)) / 2);
    right: var(--k-space-3);
    visibility: hidden;
  }

  .row:is(:hover, :focus-within, .active) :global(.more) {
    visibility: visible;
  }

  .row:is(:hover, :focus-within, .active) .lamp-slot {
    visibility: hidden;
  }

  .none {
    padding: var(--k-space-3);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .footer {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    flex: none;
    padding-top: var(--k-space-3);
    border-top: 1px solid var(--k-border);
  }

  .settings {
    height: 36px;
    color: var(--k-fg-chrome);
  }

  .settings:hover {
    color: var(--k-fg);
  }

  /* Compact sidebar: a column of tiles with lamps. Labels stay in the accessibility tree. */
  @media (max-width: 1099px) {
    .rail {
      width: var(--k-rail-width-compact);
    }

    .search {
      align-self: center;
      justify-content: center;
      width: 40px;
      padding: 0;
    }

    .item {
      justify-content: center;
      padding: 0;
    }

    .heading {
      justify-content: center;
      padding-left: 0;
    }

    .text,
    .search-text,
    .kbd,
    h2,
    .heading :global(.label),
    .none {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }

    .row :global(.more) {
      display: none;
    }

    /* The corner lamp is the only state cue here: a bezel halo keeps it apart from the tile. */
    .lamp-slot,
    .row:is(:hover, :focus-within, .active) .lamp-slot {
      position: absolute;
      top: 2px;
      left: calc(50% + 6px);
      width: auto;
      padding: 2px;
      border-radius: 50%;
      background: var(--k-bezel);
      visibility: visible;
    }

    .lamp-slot:empty {
      display: none;
    }

    .heading :global(.k-button) {
      width: var(--k-control-height-sm);
      padding: 0;
      border-color: var(--k-border-strong);
    }

    .badge {
      position: absolute;
      top: 2px;
      left: calc(50% + 4px);
    }

    /* Compact: the corner lamp is the only cue. */
    .ready {
      display: none;
    }
  }
</style>
