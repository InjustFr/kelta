<script lang="ts">
  import { paneComponent, type AnyComponent } from '$app/registry';
  import type { PaneNode, Rect } from '$lib/layout';
  import type { ProjectId, TabId } from '$lib/gen';
  import { pluginEnable } from '$lib/ipc/commands';
  import { lampOf, plugins, sessions, toasts } from '$lib/stores';
  import { Badge, Button, ErrorState, Icon, IconButton, Lamp, Menu, Spinner, type MenuItem } from '$lib/ui';

  import { prompts } from './confirm.svelte';
  import EmptyPane from './EmptyPane.svelte';
  import { lazyComponents } from './lazy.svelte';
  import { paneIcon, paneTitle, statusLabel } from './labels';
  import {
    closePaneAndStop,
    closePaneById,
    focusPaneById,
    openContent,
    splitFocused,
    toggleZoomFocused,
  } from './nav';

  interface Props {
    projectId: ProjectId;
    tabId: TabId;
    node: PaneNode;
    rect: Rect;
    visible: boolean;
    focused: boolean;
    zoomed: boolean;
  }

  let { projectId, tabId, node, rect, visible, focused, zoomed }: Props = $props();

  const content = $derived(node.content);
  const session = $derived(content.kind === 'terminal' ? sessions.get(content.session_id) : null);
  const loader = $derived(paneComponent(content.kind));
  const entry = $derived(lazyComponents.get(`pane:${content.kind}`));
  const Component = $derived(entry.component as AnyComponent | null);
  const hooksInactive = $derived(
    session?.kind.type === 'claude' && session.lifecycle === 'live' && session.claude?.hooks_active === false,
  );
  const status = $derived(session ? statusLabel(session.status) : '');

  let menu = $state<{ x: number; y: number } | null>(null);
  let boundaryKey = $state(0);

  $effect(() => {
    if (loader) lazyComponents.ensure(`pane:${content.kind}`, loader);
  });

  function focus(): void {
    if (!focused) focusPaneById(projectId, tabId, node.id);
  }

  function close(): void {
    closePaneById(projectId, tabId, node.id);
  }

  function menuItems(): MenuItem[] {
    const items: MenuItem[] = [
      { id: 'right', label: 'Split right', icon: 'columns-2' },
      { id: 'down', label: 'Split down', icon: 'rows-2' },
      { id: 'zoom', label: zoomed ? 'Unzoom' : 'Zoom', icon: zoomed ? 'minimize-2' : 'maximize-2' },
    ];
    if (session) {
      items.push({ id: 'rename', label: 'Rename session…', icon: 'pencil', separator: true });
    }
    items.push({ id: 'close', label: 'Close pane', icon: 'x', separator: true });
    if (session) {
      items.push({ id: 'stop', label: 'Stop session and close', icon: 'square', danger: true });
    }
    return items;
  }

  async function onMenu(id: string): Promise<void> {
    focus();
    switch (id) {
      case 'right':
        await splitFocused('row');
        break;
      case 'down':
        await splitFocused('column');
        break;
      case 'zoom':
        toggleZoomFocused();
        break;
      case 'rename': {
        if (!session) break;
        const name = await prompts.ask({ title: 'Rename session', label: 'Name', value: session.name });
        if (name && name.trim() && name !== session.name) {
          await sessions
            .rename(session.id, name.trim())
            .catch((err: unknown) => toasts.error(err, 'Rename failed'));
        }
        break;
      }
      case 'close':
        close();
        break;
      case 'stop':
        await closePaneAndStop(projectId, tabId, node.id);
        break;
    }
  }

  function pluginId(): string | null {
    return content.kind === 'plugin_screen' ? content.plugin_id : null;
  }

  async function disablePlugin(): Promise<void> {
    const id = pluginId();
    if (!id) return;
    try {
      await pluginEnable({ id, enabled: false });
      await plugins.load();
      close();
    } catch (err) {
      toasts.error(err, 'Disabling the plugin failed');
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="slot"
  class:hidden={!visible}
  style:left="{rect.x * 100}%"
  style:top="{rect.y * 100}%"
  style:width="{rect.w * 100}%"
  style:height="{rect.h * 100}%"
  data-testid="pane"
  data-pane-id={node.id}
  data-pane-kind={content.kind}
  data-focused={focused}
  onpointerdowncapture={focus}
>
  <section class="pane" class:focused aria-label={paneTitle(content, session)}>
    <header
      oncontextmenu={(e) => {
        e.preventDefault();
        focus();
        menu = { x: e.clientX, y: e.clientY };
      }}
      ondblclick={toggleZoomFocused}
    >
      <Icon name={paneIcon(content, session)} size={13} />
      <span class="title" data-testid="pane-title">{paneTitle(content, session)}</span>
      {#if session}
        <Lamp level={lampOf(session.attention, session.status === 'working')} />
        {#if status}<span class="status" data-testid="pane-status">{status}</span>{/if}
      {/if}
      {#if hooksInactive}
        <span class="hooks" data-testid="hooks-badge"
          ><Badge tone="warn" title="Claude status hooks are not reporting">status hooks inactive</Badge>
          <button
            type="button"
            class="fix"
            onclick={() => openContent(projectId, { content: { kind: 'diagnostics' }, placement: 'new_tab' })}
            >Fix</button
          ></span
        >
      {/if}
      <span class="spacer"></span>
      <IconButton
        icon={zoomed ? 'minimize-2' : 'maximize-2'}
        label={zoomed ? 'Unzoom pane' : 'Zoom pane'}
        size="sm"
        onclick={() => {
          focus();
          toggleZoomFocused();
        }}
      />
      <IconButton
        icon="ellipsis"
        label="Pane menu"
        size="sm"
        onclick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          focus();
          menu = { x: r.left, y: r.bottom + 2 };
        }}
      />
      <IconButton icon="x" label="Close pane" size="sm" onclick={close} data-testid="pane-close" />
    </header>
    <div class="body">
      {#key boundaryKey}
        <svelte:boundary onerror={(e) => console.error(`[kelta] pane ${content.kind} crashed`, e)}>
          {#if content.kind === 'empty'}
            <EmptyPane onclose={close} />
          {:else if Component}
            <Component {projectId} {tabId} paneId={node.id} {content} {visible} {focused} />
          {:else if entry.error}
            <ErrorState
              error={entry.error}
              title="Could not load this view"
              onretry={() => {
                lazyComponents.reset(`pane:${content.kind}`);
                if (loader) lazyComponents.ensure(`pane:${content.kind}`, loader);
              }}
            />
          {:else}
            <div class="loading" aria-busy="true"><Spinner size={16} /></div>
          {/if}
          {#snippet failed(error)}
            <ErrorState
              error={error instanceof Error ? error : String(error)}
              title={pluginId() ? `Plugin ${pluginId()} stopped working` : 'This pane crashed'}
              onretry={pluginId() ? undefined : () => boundaryKey++}
            >
              {#snippet actions()}
                {#if pluginId()}
                  <Button icon="refresh-cw" onclick={() => boundaryKey++}>Reload screen</Button>
                  <Button variant="danger" onclick={disablePlugin}>Disable plugin</Button>
                {/if}
                <Button onclick={close}>Close pane</Button>
              {/snippet}
            </ErrorState>
          {/snippet}
        </svelte:boundary>
      {/key}
    </div>
  </section>
</div>

{#if menu}
  <Menu
    items={menuItems()}
    x={menu.x}
    y={menu.y}
    label="Pane actions"
    onselect={(id) => void onMenu(id)}
    onclose={() => (menu = null)}
  />
{/if}

<style>
  .slot {
    position: absolute;
    /* The bezel shows through a 1px gap right and below each pane: panes have no borders. */
    padding: 0 var(--k-gap) var(--k-gap) 0;
    min-width: 0;
    min-height: 0;
  }

  .slot.hidden {
    display: none;
  }

  .pane {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    background: var(--k-well);
    overflow: hidden;
  }

  /* Header and terminal read as one screen; the focused pane gets an accent tick on top. */
  header {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: var(--k-pane-header-height);
    flex: none;
    padding: 0 var(--k-space-2) 0 var(--k-space-3);
    background: var(--k-well);
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-sm);
  }

  .pane.focused header {
    color: var(--k-fg);
    box-shadow: inset 0 2px 0 var(--k-accent);
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pane.focused .title {
    font-weight: var(--k-weight-strong);
  }

  .status {
    color: var(--k-fg-subtle);
    white-space: nowrap;
  }

  .spacer {
    flex: 1;
  }

  .hooks {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .fix {
    padding: 0;
    border: none;
    background: transparent;
    color: var(--k-accent);
    cursor: pointer;
  }

  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }

  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    /* Skeleton only after 150 ms (ARCHITECTURE §12.2). */
    opacity: 0;
    animation: appear 0s linear 150ms forwards;
  }

  @keyframes appear {
    to {
      opacity: 1;
    }
  }
</style>
