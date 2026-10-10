<script lang="ts">
  // Settings → Tools: add/edit/remove `[[tools]]` of the edited layer (global or project) through
  // the generic keyed-list editor, then the merged list (config layers + plugins) with an
  // install check.
  import type { SettingsSectionProps } from '$app/registry';
  import type { ToolCheck } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, tools, ui } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  import SectionForm from '../../settings/SectionForm.svelte';

  let props: SettingsSectionProps = $props();
  const { layer, projectId } = $derived(props);

  const pid = $derived(projectId ?? projects.activeId);
  const slot = $derived(pid ? tools.byProject[pid] : undefined);
  let checks = $state<Record<string, ToolCheck | string>>({});

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
  {#snippet after()}
    <section data-layer={layer} class="tools">
      <h2>Check if installed</h2>
      <p class="muted">Tools you can open from New tab or the palette, from config files or plugins.</p>
      {#if !pid}
        <EmptyState icon="folder" title="No project selected" />
      {:else if slot?.error && !slot.data}
        <ErrorState error={slot.error} onretry={() => tools.load(pid)} />
      {:else if !slot?.data}
        <Spinner />
      {:else if slot.data.length === 0}
        <EmptyState
          icon="wrench"
          title="No tools yet"
          body="Tools are programs like lazygit that open in their own pane. Install a plugin that adds some."
        >
          {#snippet actions()}
            <Button size="sm" onclick={() => ui.openSheet('plugin_install', {})}>Install plugin</Button>
          {/snippet}
        </EmptyState>
      {:else}
        <table>
          <tbody>
            {#each slot.data as t (t.id)}
              {@const c = checks[t.id]}
              <tr>
                <td><strong>{t.label}</strong> <span class="muted">{t.id}</span></td>
                <td><Badge tone="info">{t.kind}</Badge></td>
                <td>
                  <Badge>{t.source.kind === 'plugin' ? `plugin ${t.source.plugin_id}` : t.source.layer}</Badge
                  >
                </td>
                <td class="state">
                  {#if typeof c === 'string'}
                    <span class="bad">{c}</span>
                  {:else if c}
                    {#if c.installed}<span class="ok">{c.version ?? 'installed'}</span>
                    {:else}<span class="bad">missing{c.install_hint ? ` · ${c.install_hint}` : ''}</span>{/if}
                  {:else if t.installed === false}
                    <span class="bad">missing</span>
                  {/if}
                </td>
                <td><Button size="sm" variant="ghost" onclick={() => check(t.id)}>Check</Button></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  {/snippet}
</SectionForm>

<style>
  h2 {
    margin: 0;
    font-size: var(--k-font-size-lg);
  }

  .muted {
    color: var(--k-fg-muted);
  }

  table {
    width: 100%;
    margin-top: var(--k-space-4);
    border-collapse: collapse;
  }

  td {
    padding: var(--k-space-2) var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
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
