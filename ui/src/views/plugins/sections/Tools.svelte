<script lang="ts">
  // Settings → Tools: tools are personal, so the Add tool form and the `[[tools]]` editor live at the
  // Global layer only (other layers show a note and Go to Global). Then the merged list (global +
  // plugins) with an install check and key-conflict warnings.
  import { msg, t } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';
  import type { Layer, ToolCheck } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, tools, ui } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { currentPlatform } from '$lib/ui/format';

  import SettingsCard from '../../settings/layout/SettingsCard.svelte';
  import SettingsGroup from '../../settings/layout/SettingsGroup.svelte';
  import SectionForm from '../../settings/SectionForm.svelte';
  import { effectiveBindings } from '../../settings/lib/chords';
  import { useEditor } from '../../settings/lib/editor.svelte';
  import { isRecord } from '../../settings/lib/paths';
  import { toolKeyConflict } from '../../settings/lib/tools';
  import AddToolForm from './AddToolForm.svelte';

  let props: SettingsSectionProps = $props();
  const { layer, projectId } = $derived(props);

  const pid = $derived(projectId ?? projects.activeId);
  const slot = $derived(pid ? tools.byProject[pid] : undefined);
  let checks = $state<Record<string, ToolCheck | string>>({});
  let adding = $state(false);

  const editor = useEditor(() => ({ layer: props.layer, projectId: props.projectId, repoId: props.repoId }));
  const platform = currentPlatform();
  const LAYER: Record<Layer, string> = {
    default: msg('default'),
    plugin: msg('plugin'),
    global: msg('global'),
    project: msg('project'),
    repo: msg('repo'),
    runtime: msg('runtime'),
  };
  const bindings = $derived.by(() => {
    const v = editor.valueOf('keys.bindings');
    return effectiveBindings(isRecord(v) ? (v as Record<string, string[]>) : {}, platform);
  });
  const canAdd = $derived(editor.layer === 'global');
  // Nothing to dedupe against (no project, or tool_list failed) must not block Add; only a pending load does.
  const dedupeAgainst = $derived(!pid || (slot?.error && !slot.data) ? [] : (slot?.data ?? undefined));

  $effect(() => {
    if (pid && (!slot || (slot.fetchedAt === null && !slot.loading && !slot.error))) void tools.load(pid);
  });

  async function check(id: string): Promise<void> {
    try {
      checks = { ...checks, [id]: await ipc.toolCheck({ tool_id: id }) };
    } catch (e) {
      checks = { ...checks, [id]: e instanceof Error ? e.message : String(e) };
    }
  }
</script>

<SectionForm sectionId="tools" {...props}>
  {#snippet before()}
    {#if canAdd}
      <AddToolForm {editor} merged={dedupeAgainst} {bindings} bind:open={adding} />
    {:else}
      <p class="personal" data-testid="tools-personal">
        {t('Tools are personal: they live in your global settings.')}
        <Button size="sm" onclick={() => editor.target({ layer: 'global', repoId: null })}
          >{t('Go to Global')}</Button
        >
      </p>
    {/if}
  {/snippet}
  {#snippet after()}
    <SettingsGroup
      heading={t('Your tools')}
      description={t(
        'Every tool from config files or plugins. Check shows whether its program is installed.',
      )}
      data-layer={layer}
    >
      {#if !pid}
        <EmptyState icon="folder" title={t('No project selected')} />
      {:else if slot?.error && !slot.data}
        <ErrorState error={slot.error} onretry={() => tools.load(pid)} />
      {:else if !slot?.data}
        <Spinner />
      {:else if slot.data.length === 0}
        <EmptyState
          icon="wrench"
          title={t('No tools yet')}
          body={t('Add a terminal program like lazygit, or an app like Fork.')}
        >
          {#snippet actions()}
            {#if canAdd}<Button size="sm" variant="primary" onclick={() => (adding = true)}
                >{t('Add tool')}</Button
              >{/if}
            <Button size="sm" variant="ghost" onclick={() => ui.openSheet('plugin_install', {})}
              >{t('Install plugin')}</Button
            >
          {/snippet}
        </EmptyState>
      {:else}
        {#each slot.data as tool (tool.id)}
          {@const c = checks[tool.id]}
          {@const clash = tool.keybinding
            ? toolKeyConflict(tool.keybinding, tool.id, bindings, slot.data, platform)
            : null}
          <SettingsCard heading={tool.label} data-tool={tool.id}>
            {#snippet head()}
              <span class="muted">{tool.id}</span>
              <Badge tone="info">{tool.kind}</Badge>
              <Badge
                >{tool.source.kind === 'plugin'
                  ? t('plugin {id}', { id: tool.source.plugin_id })
                  : t(LAYER[tool.source.layer])}</Badge
              >
              {#if clash}<Badge tone="warn" title={clash}>{clash}</Badge>{/if}
            {/snippet}
            {#snippet actions()}
              <span class="state">
                {#if typeof c === 'string'}
                  <span class="bad">{c}</span>
                {:else if c}
                  {#if c.installed}<span class="ok">{c.version ?? t('installed')}</span>
                  {:else}<span class="bad">{t('missing')}{c.install_hint ? ` · ${c.install_hint}` : ''}</span
                    >{/if}
                {:else if tool.installed === false}
                  <span class="bad">{t('missing')}</span>
                {/if}
              </span>
              <Button size="sm" variant="ghost" onclick={() => check(tool.id)}>{t('Check')}</Button>
            {/snippet}
          </SettingsCard>
        {/each}
      {/if}
    </SettingsGroup>
  {/snippet}
</SectionForm>

<style>
  .muted {
    color: var(--k-fg-muted);
  }

  .personal {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    margin: 0;
    color: var(--k-fg-muted);
  }

  .state {
    font-size: var(--k-font-size-sm);
  }

  .ok {
    color: var(--k-ok);
  }

  .bad {
    color: var(--k-warn);
  }
</style>
