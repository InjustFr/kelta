<script lang="ts">
  // Web tool pane (SPEC §3.5, PLUGINS §2): iframe (direct or through the header-stripping proxy),
  // or an "Open in browser" card for external tools. Exit → code + log tail + Relaunch. Closing the
  // pane stops `on_close` tools.
  import { onDestroy } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import { allPanes } from '$lib/layout';
  import * as ipc from '$lib/ipc/commands';
  import { forgetWebTool, rememberWebTool, webTools } from '$lib/plugin-host/web.svelte';
  import { layout, toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';

  let { content, projectId, visible }: PaneProps<'web'> = $props();

  const instanceId = $derived(content.tool_instance_id);
  const tool = $derived(webTools[instanceId] ?? null);
  let busy = $state(false);

  function openExternal(): void {
    if (tool)
      void ipc.openExternal({ url: tool.url }).catch((e: unknown) => toasts.error(e, 'Open in browser'));
  }

  async function relaunch(): Promise<void> {
    if (!tool) return;
    busy = true;
    try {
      await ipc.toolClose({ instance_id: instanceId }).catch(() => {});
      forgetWebTool(instanceId);
      // shortcut: relaunch opens a new tab (tool_open cannot target this pane); upgrade when it can.
      const handle = await ipc.toolOpen({
        project_id: tool.projectId,
        tool_id: tool.toolId,
        ctx: {
          repo_id: null,
          cwd: null,
          session_id: null,
          work_item_id: null,
          ticket: null,
          review: null,
          extra: {},
        },
        placement: 'new_tab',
      });
      rememberWebTool(handle, {
        toolId: tool.toolId,
        label: tool.label,
        projectId: tool.projectId,
        lifecycle: tool.lifecycle,
      });
    } catch (e) {
      toasts.error(e, `Relaunch ${tool.label}`);
    } finally {
      busy = false;
    }
  }

  onDestroy(() => {
    // Unmounted by a tab switch → still in the layout; removed → the pane was closed.
    const id = instanceId;
    const l = layout.get(projectId);
    const stillThere = l?.tabs.some((t) =>
      allPanes(t.root).some((p) => p.content.kind === 'web' && p.content.tool_instance_id === id),
    );
    const t = webTools[id];
    if (stillThere || (t && t.lifecycle !== 'on_close')) return;
    forgetWebTool(id);
    void ipc.toolClose({ instance_id: id }).catch(() => {});
  });
</script>

<div class="web" data-pane-kind={content.kind}>
  {#if !tool}
    <EmptyState
      icon="globe"
      title="This web tool is not running"
      body="It was stopped, or Kelta restarted. Open it again from the tool picker."
    />
  {:else if tool.exited}
    <div class="exited">
      <p><strong>{tool.label}</strong> exited with code {tool.exited.code}</p>
      {#if tool.exited.log}<pre class="k-selectable">{tool.exited.log}</pre>{/if}
      <Button icon="refresh-cw" loading={busy} onclick={relaunch}>Relaunch</Button>
    </div>
  {:else if tool.embed === 'external'}
    <EmptyState icon="globe" title={`${tool.label} opens in your browser`}>
      {#snippet actions()}
        <Button variant="primary" onclick={openExternal}>Open in browser</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="bar">
      <span class="label">{tool.label}</span>
      <Button size="sm" variant="ghost" icon="refresh-cw" loading={busy} onclick={relaunch}>Restart</Button>
      <Button size="sm" variant="ghost" onclick={openExternal}>Open in browser</Button>
    </div>
    {#if visible}
      <!-- Loopback tool origin (cross-origin to the app): its own storage, no access to Kelta. -->
      <iframe
        sandbox="allow-scripts allow-forms allow-same-origin allow-downloads allow-modals"
        src={tool.url}
        title={tool.label}
        referrerpolicy="no-referrer"
      ></iframe>
    {/if}
  {/if}
</div>

<style>
  .web {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    padding: var(--k-space-1) var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
    background: var(--k-bg-elev);
  }

  .label {
    flex: 1;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  iframe {
    flex: 1;
    width: 100%;
    border: 0;
    background: #fff;
  }

  .exited {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    padding: var(--k-space-5);
  }

  pre {
    margin: 0;
    max-height: 50vh;
    overflow: auto;
    padding: var(--k-space-3);
    background: var(--k-bg-sunken);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    white-space: pre-wrap;
  }
</style>
