<script lang="ts">
  // Install a plugin (PLUGINS §4): plugin_inspect shows the manifest, its permissions in plain
  // words, warnings and the SHA-256; Install grants exactly the listed permissions.
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type { PluginInstallPreview } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError, type IpcError } from '$lib/ipc/transport';
  import { plugins, toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Sheet from '$lib/ui/Sheet.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  let { onclose, source: initial }: SheetProps = $props();

  let source = $state(untrack(() => (typeof initial === 'string' ? initial : '')));
  let preview = $state<PluginInstallPreview | null>(null);
  let error = $state<IpcError | null>(null);
  let busy = $state(false);

  async function inspect(e?: Event): Promise<void> {
    e?.preventDefault();
    if (!source.trim()) return;
    busy = true;
    error = null;
    preview = null;
    try {
      preview = await ipc.pluginInspect({ source: source.trim() });
    } catch (err) {
      error = toIpcError('plugin_install', err);
    } finally {
      busy = false;
    }
  }

  async function install(): Promise<void> {
    if (!preview) return;
    busy = true;
    try {
      const info = await ipc.pluginInstall({
        source: source.trim(),
        sha256: preview.sha256,
        grant: preview.permissions.map((p) => p.permission),
      });
      toasts.info(`Installed ${info.name} ${info.version}`);
      void plugins.load();
      onclose();
    } catch (err) {
      error = toIpcError('plugin_install', err);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet title="Install plugin" {onclose}>
  <div class="install">
    <form onsubmit={inspect}>
      <TextInput
        bind:value={source}
        label="Source"
        hint="A plugin directory, a git URL (optionally #tag) or a .tar.gz"
        placeholder="~/code/my-plugin, a git URL or a .tgz file"
      />
      <Button type="submit" loading={busy && !preview} disabled={!source.trim()}>Inspect</Button>
    </form>

    {#if error}
      <ErrorState {error} title="Cannot install this plugin" />
    {/if}

    {#if preview}
      {@const m = preview.manifest}
      <section class="preview" aria-label="Plugin details">
        <h3>{m.name} <span class="muted">{m.version}</span></h3>
        <p>{m.description}</p>
        <p class="muted"><code>{m.id}</code>&ensp;{m.author}&ensp;{m.license}</p>

        <h4>This plugin will be allowed to</h4>
        {#if preview.permissions.length === 0}
          <p class="muted">Nothing beyond showing its own screens.</p>
        {:else}
          <ul class="perms" data-testid="plugin-permissions">
            {#each preview.permissions as p (p.permission)}
              <li><span>{p.description}</span> <code>{p.permission}</code></li>
            {/each}
          </ul>
        {/if}

        {#if preview.warnings.length}
          <ul class="warnings" role="alert">
            {#each preview.warnings as w (w)}<li>{w}</li>{/each}
          </ul>
        {/if}

        <p class="sha">SHA-256 <code class="k-selectable">{preview.sha256}</code></p>
      </section>
    {/if}
  </div>

  {#snippet actions()}
    <Button onclick={onclose}>Cancel</Button>
    <Button variant="primary" disabled={!preview} loading={busy && !!preview} onclick={install}>
      Install
    </Button>
  {/snippet}
</Sheet>

<style>
  .install {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
  }

  form {
    display: flex;
    align-items: flex-end;
    gap: var(--k-space-3);
  }

  form :global(.k-field) {
    flex: 1;
  }

  h3,
  h4,
  p {
    margin: 0 0 var(--k-space-2);
  }

  .muted {
    color: var(--k-fg-muted);
    font-weight: normal;
  }

  ul {
    margin: 0 0 var(--k-space-3);
    padding-left: var(--k-space-5);
  }

  .perms code {
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
  }

  .warnings {
    color: var(--k-warn);
  }

  .sha code {
    font-size: var(--k-font-size-xs);
    overflow-wrap: anywhere;
  }
</style>
