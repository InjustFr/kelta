<script lang="ts">
  // The bar above the panes: `project › row title` and the `+` menu. The rail lists the tabs.
  // shortcut: the context bar of #248 replaces this; until then it only names where you are.
  import { dispatch } from '$lib/actions';
  import type { ProjectId, ToolInfo } from '$lib/gen';
  import { activeTab } from '$lib/layout';
  import { layout, projects, toasts, tools, ui } from '$lib/stores';
  import { Button, Menu, type MenuItem } from '$lib/ui';

  import { chordFor } from './labels';
  import { rowLabel } from './nav';

  interface Props {
    projectId: ProjectId;
  }

  let { projectId }: Props = $props();

  const current = $derived(layout.get(projectId));
  const active = $derived(current ? activeTab(current) : null);
  const toolList = $derived(tools.list(projectId));
  const project = $derived(projects.byId(projectId));

  let plusMenu = $state<{ x: number; y: number } | null>(null);

  $effect(() => {
    // Tools feed the "+" menu; they are cached per project.
    if (tools.byProject[projectId] === undefined) void tools.load(projectId);
  });

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
      // The project's own tabs; the bare actions open the rail's cross-project views.
      else if (id === 'tickets') await dispatch('tickets.open', { project_id: projectId });
      else if (id === 'reviews') await dispatch('reviews.open', { project_id: projectId });
      else if (id.startsWith('tool:')) await dispatch('tools.open', { tool_id: id.slice(5) });
    } catch (err) {
      toasts.error(err, 'Could not open the view');
    }
  }
</script>

<div class="tabbar" data-testid="tabbar">
  <span
    class="context"
    data-testid="tabbar-context"
    style:--project-color={project?.color ?? 'var(--k-border-strong)'}
  >
    <span class="project">{project?.name ?? ''}</span>
    <span class="sep" aria-hidden="true">›</span>
    <span class="title">{active ? rowLabel(active) : ''}</span>
  </span>
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
  }

  /* ▌project › title: the project colour bar, then where you are. */
  .context {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
    padding-left: var(--k-space-3);
    box-shadow: inset 3px 0 0 var(--project-color);
    white-space: nowrap;
  }

  .project {
    flex: none;
    color: var(--k-fg-muted);
    font-weight: var(--k-weight-medium);
  }

  .sep {
    color: var(--k-fg-subtle);
  }

  .title {
    overflow: hidden;
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
    text-overflow: ellipsis;
  }

  .tabbar :global(.plus) {
    flex: none;
    margin-left: auto;
    color: var(--k-fg-chrome);
  }
</style>
