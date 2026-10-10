<script lang="ts">
  // Plugin screen host (SPEC §3.6, ARCHITECTURE §11.3): a sandboxed iframe created when the pane is
  // shown and destroyed when hidden (unless the screen is keep_alive). Each iframe gets a fresh
  // screen instance from plugin_screen_open, closed again with the iframe.
  import { onMount, untrack } from 'svelte';

  import type { PaneProps } from '$app/registry';
  import type { ScreenOpenResult } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError, type IpcError } from '$lib/ipc/transport';
  import { connectScreen, onThemeChange, themeTokens, type ScreenBridge } from '$lib/plugin-host/bridge';
  import { plugins, settings, toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  let { content, projectId, visible }: PaneProps<'plugin_screen'> = $props();

  // Primitive copies so layout.changed (new content objects) does not reopen the screen.
  const pluginId = $derived(content.plugin_id);
  const screenId = $derived(content.screen_id);
  const pluginName = $derived(plugins.plugins.data?.find((p) => p.id === pluginId)?.name ?? pluginId);
  const paramsJson = $derived(JSON.stringify(content.params ?? null));
  const def = $derived(
    plugins.plugins.data
      ?.find((p) => p.id === pluginId)
      ?.contributes.screens.find((s) => s.id === screenId) ?? null,
  );
  const live = $derived(visible || (def?.keep_alive ?? false));

  let opened = $state<ScreenOpenResult | null>(null);
  let error = $state<IpcError | null>(null);
  let reloads = $state(0);
  let iframe = $state<HTMLIFrameElement>();
  let bridge = $state<ScreenBridge | null>(null);

  onMount(() => {
    if (plugins.plugins.fetchedAt === null) void plugins.load();
    // The instance opened with the pane (backend open_screen) is replaced by our own per iframe.
    void ipc.pluginScreenClose({ instance_id: content.instance_id }).catch(() => {});
  });

  $effect(() => {
    if (!live) return;
    void reloads;
    let alive = true;
    let instance: string | null = null;
    error = null;
    ipc
      .pluginScreenOpen({
        plugin_id: pluginId,
        screen_id: screenId,
        project_id: projectId,
        params: untrack(() => content.params),
      })
      .then(
        (r) => {
          instance = r.instance_id;
          if (alive) opened = r;
          else void ipc.pluginScreenClose({ instance_id: r.instance_id }).catch(() => {});
        },
        (e: unknown) => {
          if (alive) error = toIpcError('plugin_screen_open', e);
        },
      );
    return () => {
      alive = false;
      opened = null;
      if (instance) void ipc.pluginScreenClose({ instance_id: instance }).catch(() => {});
    };
  });

  $effect(() => {
    const frame = iframe;
    const o = opened;
    if (!frame || !o) return;
    const b = connectScreen(
      frame,
      {
        instance: o.instance_id,
        plugin: pluginId,
        project: projectId,
        params: untrack(() => content.params),
      },
      () => themeTokens(settings.theme),
    );
    const offEvents = plugins.onScreenEvent(o.instance_id, (name, payload) =>
      b.push({ type: 'event', name, payload }),
    );
    const offTheme = onThemeChange(() => b.push({ type: 'theme', tokens: themeTokens(settings.theme) }));
    bridge = b;
    return () => {
      offEvents();
      offTheme();
      b.destroy();
      bridge = null;
    };
  });

  $effect(() => {
    bridge?.push({ type: 'visibility', visible });
  });

  $effect(() => {
    const params = JSON.parse(paramsJson) as unknown;
    untrack(() => bridge)?.push({ type: 'params', params });
  });

  async function disable(): Promise<void> {
    try {
      await ipc.pluginEnable({ id: pluginId, enabled: false });
      toasts.info(`Plugin ${pluginId} disabled`);
      void plugins.load();
    } catch (e) {
      toasts.error(e, 'Disable plugin');
    }
  }
</script>

<div class="screen" data-pane-kind={content.kind} data-plugin={pluginId}>
  {#if error}
    <ErrorState
      error={{ ...error, message: `${pluginId}/${screenId}: ${error.message}` }}
      title={`${pluginName} screen failed`}
    >
      {#snippet actions()}
        <Button icon="refresh-cw" onclick={() => reloads++}>Reload screen</Button>
        <Button variant="danger" onclick={disable}>Disable plugin</Button>
      {/snippet}
    </ErrorState>
  {:else if live && opened}
    <!-- No allow-same-origin: the screen gets an opaque origin (no storage, no Tauri IPC). -->
    <iframe
      bind:this={iframe}
      sandbox="allow-scripts allow-forms"
      src={opened.url}
      title={def?.title ?? `${pluginId}/${screenId}`}
      referrerpolicy="no-referrer"
      class:hidden={!visible}
    ></iframe>
  {:else if live}
    <div class="loading"><Spinner label="Opening screen" /></div>
  {/if}
</div>

<style>
  .screen {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  iframe {
    flex: 1;
    width: 100%;
    border: 0;
    background: var(--k-well);
  }

  iframe.hidden {
    display: none;
  }

  .loading {
    display: flex;
    flex: 1;
    padding: var(--k-space-5);
  }
</style>
