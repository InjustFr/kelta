<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { tabHeaderRegistry, type AnyComponent } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { ProjectId, TabId } from '$lib/gen';
  import { activeTab, findTab, nodeAt, resizeGutter, updateTab } from '$lib/layout';
  import { layout, projects } from '$lib/stores';
  import { Button, EmptyState, ErrorState, Spinner } from '$lib/ui';

  import WelcomePane from '../views/welcome/WelcomePane.svelte';
  import { applyZoom, dropTarget, layoutGeometry, type Gutter as GutterSpec } from './geometry';
  import GutterView from './Gutter.svelte';
  import { chordFor } from './labels';
  import { lazyComponents } from './lazy.svelte';
  import { attachTab, detachTab } from './nav';
  import PaneHost from './PaneHost.svelte';
  import { paneDrag } from './paneDrag.svelte';
  import { spawnTemplate } from './spawn';

  interface Props {
    projectId: ProjectId;
    /** A detached window's tab (#342); the main window shows the active tab. */
    tabId?: TabId;
  }

  let { projectId, tabId }: Props = $props();

  const current = $derived(layout.get(projectId));
  const failure = $derived(layout.errors[projectId] ?? null);
  const tab = $derived(current ? (tabId ? findTab(current, tabId) : activeTab(current)) : null);
  const geometry = $derived(
    tab ? applyZoom(layoutGeometry(tab.root), tab.zoomed_pane) : { panes: [], gutters: [] },
  );
  const onlyHome = $derived(projects.list.every((p) => p.builtin));

  let width = $state(0);
  let height = $state(0);

  const BriefComponent = $derived(lazyComponents.get('tabheader:brief').component as AnyComponent | null);

  $effect(() => {
    if (!current && !failure) void layout.ensure(projectId).catch(() => {});
  });

  $effect(() => {
    if (tab?.work_item_id) lazyComponents.ensure('tabheader:brief', tabHeaderRegistry.brief);
  });

  const dragging = $derived(paneDrag.current?.projectId === projectId ? paneDrag.current : null);

  /** Drop indicator under the pointer; none over the dragged pane itself. */
  function dragOver(e: DragEvent): void {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const shown = geometry.panes.filter((p) => !tab?.zoomed_pane || p.pane.id === tab.zoomed_pane);
    const hit = dropTarget(shown, (e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height);
    paneDrag.over =
      hit && hit.pane.id !== dragging?.paneId
        ? { paneId: hit.pane.id, zone: hit.zone, area: hit.area }
        : null;
    if (paneDrag.over) e.preventDefault();
  }

  function drop(e: DragEvent): void {
    e.preventDefault();
    const over = paneDrag.over;
    if (over && tab) paneDrag.drop(projectId, { tabId: tab.id, paneId: over.paneId, zone: over.zone });
    else paneDrag.end();
  }

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

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape' && dragging) paneDrag.end();
  }}
/>

<div class="workspace" data-testid="workspace" data-project-id={projectId}>
  {#if failure && !current}
    <ErrorState
      error={failure}
      title={t('Could not load the layout')}
      onretry={() => layout.load(projectId).catch(() => {})}
    />
  {:else if !current}
    <div class="loading" aria-busy="true"><Spinner size={18} /></div>
  {:else if !tab}
    {#if onlyHome && projectId === projects.home?.id}
      <WelcomePane {projectId} />
    {:else if projectId === projects.home?.id}
      <EmptyState
        icon="square-terminal"
        title={t('Nothing open in Home')}
        body={t('Home runs terminals in your home folder, outside any project.')}
      >
        {#snippet actions()}
          <Button variant="primary" icon="square-terminal" onclick={() => spawnTemplate(projectId, 'shell')}
            >{t('Open a terminal')}</Button
          >
          <Button icon="plus" chord={chordFor('session.new')} onclick={() => dispatch('session.new')}
            >{t('New session…')}</Button
          >
        {/snippet}
      </EmptyState>
    {:else}
      <EmptyState
        icon="square-terminal"
        title={t('Nothing open in {project} yet', {
          project: projects.byId(projectId)?.name ?? t('this project'),
        })}
        body={t("A session is a terminal running Claude Code, nvim or a shell in this project's folder.")}
      >
        {#snippet actions()}
          <Button variant="primary" icon="play" onclick={() => spawnTemplate(projectId, 'claude+editor')}
            >{t('Start Claude and nvim')}</Button
          >
          <Button icon="plus" chord={chordFor('session.new')} onclick={() => dispatch('session.new')}
            >{t('New session…')}</Button
          >
          <Button icon="ticket" chord={chordFor('tickets.open')} onclick={() => dispatch('tickets.open')}
            >{t('Tickets')}</Button
          >
        {/snippet}
      </EmptyState>
    {/if}
  {:else if tab.detached && !tabId}
    <EmptyState
      icon="app-window"
      title={t('This tab is in its own window')}
      body={t('Its sessions keep running there. Closing that window brings the tab back here.')}
    >
      {#snippet actions()}
        <Button variant="primary" icon="app-window" onclick={() => detachTab(projectId, tab.id)}
          >{t('Show window')}</Button
        >
        <Button onclick={() => attachTab(projectId, tab.id)}>{t('Move back here')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    {#if tab.work_item_id && BriefComponent}
      <BriefComponent {projectId} workItemId={tab.work_item_id} />
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
      {#if dragging}
        <!-- Catches the drag over every pane (web views included) while a pane header is dragged. -->
        <div
          class="drop-layer"
          role="presentation"
          data-testid="pane-drop-layer"
          ondragover={dragOver}
          ondragleave={() => (paneDrag.over = null)}
          ondrop={drop}
        >
          {#if paneDrag.over}
            {@const a = paneDrag.over.area}
            <div
              class="drop-indicator"
              data-testid="pane-drop-indicator"
              data-zone={paneDrag.over.zone}
              style:left="{a.x * 100}%"
              style:top="{a.y * 100}%"
              style:width="{a.w * 100}%"
              style:height="{a.h * 100}%"
            ></div>
          {/if}
        </div>
      {/if}
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
    margin: calc(var(--k-gap) / 2);
    background: var(--k-bezel);
  }

  .drop-layer {
    position: absolute;
    inset: 0;
    z-index: 3; /* above the gutters */
  }

  .drop-indicator {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--k-accent);
    border-radius: var(--k-radius-screen);
    background: color-mix(in oklab, var(--k-accent) 18%, transparent);
    pointer-events: none;
  }

  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    animation: k-show var(--k-duration) var(--k-ease) var(--k-delay-show) backwards;
  }
</style>
