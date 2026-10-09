<script lang="ts">
  import { tabHeaderRegistry, type AnyComponent } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { ProjectId } from '$lib/gen';
  import { activeTab, nodeAt, resizeGutter, updateTab } from '$lib/layout';
  import { layout, projects, ui } from '$lib/stores';
  import { Button, EmptyState, ErrorState, Spinner } from '$lib/ui';

  import { applyZoom, layoutGeometry, type Gutter as GutterSpec } from './geometry';
  import GutterView from './Gutter.svelte';
  import { lazyComponents } from './lazy.svelte';
  import PaneHost from './PaneHost.svelte';

  interface Props {
    projectId: ProjectId;
  }

  let { projectId }: Props = $props();

  const current = $derived(layout.get(projectId));
  const failure = $derived(layout.errors[projectId] ?? null);
  const tab = $derived(current ? activeTab(current) : null);
  const geometry = $derived(
    tab ? applyZoom(layoutGeometry(tab.root), tab.zoomed_pane) : { panes: [], gutters: [] },
  );
  const onlyHome = $derived(projects.list.every((p) => p.builtin));

  let width = $state(0);
  let height = $state(0);

  const HeaderComponent = $derived(lazyComponents.get('tabheader:work').component as AnyComponent | null);

  $effect(() => {
    if (!current && !failure) void layout.ensure(projectId).catch(() => {});
  });

  $effect(() => {
    if (tab?.work_item_id) lazyComponents.ensure('tabheader:work', tabHeaderRegistry.work);
  });

  function ratioOf(g: GutterSpec): number {
    const node = tab ? nodeAt(tab.root, g.path) : null;
    return node && node.type === 'split' ? (node.ratios[g.index] ?? 0.5) : 0.5;
  }

  function resize(g: GutterSpec, delta: number): void {
    const tabId = tab?.id;
    if (!tabId) return;
    layout.update(projectId, (l) =>
      updateTab(l, tabId, (t) => ({ ...t, root: resizeGutter(t.root, g.path, g.index, delta) })),
    );
  }
</script>

<div class="workspace" data-testid="workspace" data-project-id={projectId}>
  {#if failure && !current}
    <ErrorState
      error={failure}
      title="Could not load the layout"
      onretry={() => layout.load(projectId).catch(() => {})}
    />
  {:else if !current}
    <div class="loading" aria-busy="true"><Spinner size={18} /></div>
  {:else if !tab}
    <EmptyState
      icon="square-terminal"
      title="Nothing open in this project"
      body="Start a session, open your tickets or open a tool."
    >
      {#snippet actions()}
        <Button variant="primary" icon="plus" onclick={() => dispatch('session.new')}>New session</Button>
        <Button icon="ticket" onclick={() => dispatch('tickets.open')}>Tickets</Button>
        {#if onlyHome}
          <Button icon="folder-plus" onclick={() => ui.openSheet('project_new')}
            >Create project from folder</Button
          >
        {/if}
      {/snippet}
    </EmptyState>
  {:else}
    {#if tab.work_item_id && HeaderComponent}
      <div class="tab-header" data-testid="tab-header">
        <HeaderComponent {projectId} tabId={tab.id} workItemId={tab.work_item_id} />
      </div>
    {/if}
    <div class="panes" bind:clientWidth={width} bind:clientHeight={height} data-testid="panes">
      {#each geometry.panes as placed (placed.pane.id)}
        {@const zoomed = tab.zoomed_pane === placed.pane.id}
        <PaneHost
          {projectId}
          tabId={tab.id}
          node={placed.pane}
          rect={placed.rect}
          visible={!tab.zoomed_pane || zoomed}
          focused={tab.focused_pane === placed.pane.id}
          {zoomed}
        />
      {/each}
      {#each geometry.gutters as g (`${g.path.join('.')}:${g.index}`)}
        <GutterView
          gutter={g}
          areaWidth={width}
          areaHeight={height}
          value={ratioOf(g)}
          onresize={(d) => resize(g, d)}
        />
      {/each}
    </div>
  {/if}
</div>

<style>
  .workspace {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .panes {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--k-bg-sunken);
  }

  .tab-header {
    flex: none;
    border-bottom: 1px solid var(--k-border);
  }

  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    opacity: 0;
    animation: appear 0s linear 150ms forwards;
  }

  @keyframes appear {
    to {
      opacity: 1;
    }
  }
</style>
