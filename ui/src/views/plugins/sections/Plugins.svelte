<script lang="ts">
  // Settings → Plugins: install, enable/disable, re-grant after a manifest update added
  // permissions, uninstall. Plugin settings (`plugins.<id>`) render in the schema form.
  import { t } from '$lib/i18n/index.svelte';
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

  import SettingsCard from '../../settings/layout/SettingsCard.svelte';
  import SettingsGroup from '../../settings/layout/SettingsGroup.svelte';
  import SectionForm from '../../settings/SectionForm.svelte';

  let props: SettingsSectionProps = $props();
  const layer = $derived(props.layer);

  let removing = $state<PluginInfo | null>(null);

  $effect(() => {
    if (plugins.plugins.fetchedAt === null && !plugins.plugins.loading && !plugins.plugins.error)
      void plugins.load();
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
    await run(t('Uninstall {name}', { name: p.name }), () => ipc.pluginUninstall({ id: p.id }));
  }
</script>

<SectionForm sectionId="plugins" {...props}>
  {#snippet after()}
    <SettingsGroup heading={t('Installed plugins')} data-layer={layer}>
      {#snippet actions()}
        <Button size="sm" icon="plus" onclick={() => ui.openSheet('plugin_install')}
          >{t('Install plugin…')}</Button
        >
      {/snippet}
      {#if plugins.plugins.error && !plugins.plugins.data}
        <ErrorState error={plugins.plugins.error} onretry={() => plugins.load()} />
      {:else if !plugins.plugins.data}
        <Spinner />
      {:else if plugins.plugins.data.length === 0}
        <EmptyState
          icon="puzzle"
          title={t('No plugins installed')}
          body={t('Install one from a directory, a git URL or a tarball.')}
        />
      {:else}
        {#each plugins.plugins.data as p (p.id)}
          {@const newPerms = missing(p)}
          <SettingsCard heading={p.name} data-plugin={p.id}>
            {#snippet head()}
              <span class="muted">{p.id} {p.version}</span>
              {#if p.dev}<Badge tone="info">{t('dev')}</Badge>{/if}
            {/snippet}
            {#snippet actions()}
              <Toggle
                checked={p.enabled}
                label={t('Enabled')}
                onchange={(on) =>
                  run(t('Enable {name}', { name: p.name }), () =>
                    ipc.pluginEnable({ id: p.id, enabled: on }),
                  )}
              />
              <Button size="sm" variant="ghost" onclick={() => (removing = p)}>{t('Uninstall')}</Button>
            {/snippet}
            {#if p.description}<p class="muted">{p.description}</p>{/if}
            {#each p.problems as problem (problem)}<p class="problem" role="alert">{problem}</p>{/each}
            {#if newPerms.length}
              <div class="regrant" role="status">
                <span
                  >{t('An update asks for new permissions: {perms}. They stay off until granted.', {
                    perms: newPerms.join(', '),
                  })}</span
                >
                <Button
                  size="sm"
                  onclick={() =>
                    run(t('Grant {name}', { name: p.name }), () =>
                      ipc.pluginGrant({ id: p.id, permissions: p.permissions }),
                    )}
                >
                  {t('Grant')}
                </Button>
              </div>
            {/if}
          </SettingsCard>
        {/each}
      {/if}
    </SettingsGroup>
  {/snippet}
</SectionForm>

{#if removing}
  {@const p = removing}
  <Dialog title={t('Uninstall {name}?', { name: p.name })} tone="danger" onclose={() => (removing = null)}>
    <p>{t('Its files are deleted and its tools, triggers and screens disappear.')}</p>
    {#snippet actions()}
      <Button onclick={() => (removing = null)}>{t('Cancel')}</Button>
      <Button variant="danger" onclick={() => uninstall(p)}>{t('Uninstall')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  p {
    margin: 0;
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .problem,
  .regrant {
    color: var(--k-warn);
  }

  .regrant {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--k-space-3);
  }
</style>
