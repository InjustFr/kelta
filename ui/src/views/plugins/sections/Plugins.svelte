<script lang="ts">
  // Settings → Plugins: install, enable/disable, re-grant after a manifest update added
  // permissions, uninstall. Plugin settings (`plugins.<id>`) render in the schema form.
  import type { SettingsSectionProps } from '$app/registry';
  import type { PluginInfo } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { plugins, toasts, ui } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dialog from '$lib/ui/Dialog.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';

  let { layer }: SettingsSectionProps = $props();

  let removing = $state<PluginInfo | null>(null);

  $effect(() => {
    if (plugins.plugins.fetchedAt === null && !plugins.plugins.loading) void plugins.load();
  });

  const missing = (p: PluginInfo): string[] => p.permissions.filter((x) => !p.granted.includes(x));

  async function run(label: string, f: () => Promise<unknown>): Promise<void> {
    try {
      await f();
    } catch (e) {
      toasts.error(e, label);
    }
    void plugins.load();
  }

  async function uninstall(p: PluginInfo): Promise<void> {
    removing = null;
    await run(`Uninstall ${p.name}`, () => ipc.pluginUninstall({ id: p.id }));
  }
</script>

<section data-layer={layer} class="plugins">
  <header>
    <h2>Plugins</h2>
    <Button icon="plus" onclick={() => ui.openSheet('plugin_install')}>Install plugin…</Button>
  </header>

  {#if plugins.plugins.error && !plugins.plugins.data}
    <ErrorState error={plugins.plugins.error} onretry={() => plugins.load()} />
  {:else if !plugins.plugins.data}
    <Spinner />
  {:else if plugins.plugins.data.length === 0}
    <EmptyState
      icon="puzzle"
      title="No plugins installed"
      body="Install one from a directory, a git URL or a tarball."
    />
  {:else}
    <ul>
      {#each plugins.plugins.data as p (p.id)}
        {@const newPerms = missing(p)}
        <li data-plugin={p.id}>
          <div class="head">
            <strong>{p.name}</strong>
            <span class="muted">{p.id} {p.version}</span>
            {#if p.dev}<Badge tone="info">dev</Badge>{/if}
            <span class="grow"></span>
            <Toggle
              checked={p.enabled}
              label="Enabled"
              onchange={(on) => run(`Enable ${p.name}`, () => ipc.pluginEnable({ id: p.id, enabled: on }))}
            />
            <Button size="sm" variant="ghost" onclick={() => (removing = p)}>Uninstall</Button>
          </div>
          {#if p.description}<p class="muted">{p.description}</p>{/if}
          {#each p.problems as problem (problem)}<p class="problem" role="alert">{problem}</p>{/each}
          {#if newPerms.length}
            <div class="regrant" role="status">
              <span
                >An update asks for new permissions: <code>{newPerms.join(', ')}</code>. They stay off until
                granted.</span
              >
              <Button
                size="sm"
                onclick={() =>
                  run(`Grant ${p.name}`, () => ipc.pluginGrant({ id: p.id, permissions: p.permissions }))}
              >
                Grant
              </Button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if removing}
  {@const p = removing}
  <Dialog title={`Uninstall ${p.name}?`} tone="danger" onclose={() => (removing = null)}>
    <p>Its files are deleted and its tools, triggers and screens disappear.</p>
    {#snippet actions()}
      <Button onclick={() => (removing = null)}>Cancel</Button>
      <Button variant="danger" onclick={() => uninstall(p)}>Uninstall</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .plugins header,
  .head,
  .regrant {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  h2 {
    flex: 1;
    margin: 0;
    font-size: var(--k-font-size-lg);
  }

  ul {
    list-style: none;
    margin: var(--k-space-4) 0 0;
    padding: 0;
  }

  li {
    padding: var(--k-space-3) 0;
    border-bottom: 1px solid var(--k-border);
  }

  p {
    margin: var(--k-space-2) 0 0;
  }

  .grow {
    flex: 1;
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .problem,
  .regrant {
    color: var(--k-warn);
  }

  .regrant {
    justify-content: space-between;
    margin-top: var(--k-space-2);
  }
</style>
