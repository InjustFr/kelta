<script lang="ts">
  // Settings → Tools: tools are personal, so the Add tool form and the `[[tools]]` editor live at the
  // Global layer only (other layers show a note and Go to Global). Then the merged list (global +
  // plugins) with an install check and key-conflict warnings.
  import type { SettingsSectionProps } from '$app/registry';
  import type { ToolCheck } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, tools, ui } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { currentPlatform } from '$lib/ui/format';

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
        Tools are personal: they live in your global settings.
        <Button size="sm" onclick={() => editor.target({ layer: 'global', repoId: null })}
          >Go to Global</Button
        >
      </p>
    {/if}
  {/snippet}
  {#snippet after()}
    <section data-layer={layer} class="tools">
      <h2>Check if installed</h2>
      <p class="muted">
        Every tool from config files or plugins: programs like lazygit that open in a pane, or apps like Fork.
      </p>
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
          body="Add a terminal program like lazygit, or an app like Fork."
        >
          {#snippet actions()}
            {#if canAdd}<Button size="sm" variant="primary" onclick={() => (adding = true)}>Add tool</Button
              >{/if}
            <Button size="sm" variant="ghost" onclick={() => ui.openSheet('plugin_install', {})}
              >Install plugin</Button
            >
          {/snippet}
        </EmptyState>
      {:else}
        <table>
          <tbody>
            {#each slot.data as t (t.id)}
              {@const c = checks[t.id]}
              {@const clash = t.keybinding
                ? toolKeyConflict(t.keybinding, t.id, bindings, slot.data, platform)
                : null}
              <tr>
                <td>
                  <strong>{t.label}</strong> <span class="muted">{t.id}</span>
                  {#if clash}<Badge tone="warn" title={clash}>{clash}</Badge>{/if}
                </td>
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

  .personal {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    margin: 0;
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
