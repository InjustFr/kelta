<script lang="ts">
  // Tool picker sheet (SPEC §3.5): filter, Enter/click opens. An open that fails on a missing
  // binary shows its install hint and "Check again" (tool_check).
  import { dispatch } from '$lib/actions';
  import type { SheetProps } from '$app/registry';
  import type { ToolCheck, ToolInfo } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, tools } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Sheet from '$lib/ui/Sheet.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  import { focusedWorkItem } from '../work/fixloop.svelte';
  import { openTool } from './actions';

  // `query`/`checks` seed the sheet when `tools.open` found the requested tool missing.
  let { onclose, projectId: given, query: q0, checks: c0 }: SheetProps = $props();

  const projectId = $derived((typeof given === 'string' ? given : null) ?? projects.activeId);
  const slot = $derived(projectId ? tools.byProject[projectId] : undefined);
  // svelte-ignore state_referenced_locally
  let query = $state(typeof q0 === 'string' ? q0 : '');
  // svelte-ignore state_referenced_locally
  let checks = $state<Record<string, ToolCheck>>((c0 as Record<string, ToolCheck> | undefined) ?? {});

  const shown = $derived(
    (slot?.data ?? []).filter((t) =>
      `${t.label} ${t.id} ${t.description ?? ''}`.toLowerCase().includes(query.trim().toLowerCase()),
    ),
  );

  $effect(() => {
    if (projectId && (!slot || (slot.fetchedAt === null && !slot.loading && !slot.error)))
      void tools.load(projectId);
  });

  function installed(t: ToolInfo): boolean {
    return checks[t.id]?.installed ?? t.installed !== false;
  }

  async function check(t: ToolInfo): Promise<void> {
    try {
      checks = { ...checks, [t.id]: await ipc.toolCheck({ tool_id: t.id }) };
    } catch (e) {
      checks = { ...checks, [t.id]: { installed: false, version: null, install_hint: String(e) } };
    }
  }

  async function open(t: ToolInfo): Promise<void> {
    if (!projectId) return;
    // The active tab's work item, as `tools.open` sends: only meaningful in the shown project.
    const workItemId = projectId === projects.activeId ? focusedWorkItem() : null;
    const r = await openTool(projectId, t, { work_item_id: workItemId });
    if (r === true) onclose();
    else if (r) checks = { ...checks, [t.id]: r };
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && shown[0]) void open(shown[0]);
  }
</script>

<Sheet title="Open tool" {onclose}>
  <div class="picker">
    <TextInput bind:value={query} placeholder="Filter tools" aria-label="Filter tools" {onkeydown} />
    {#if !projectId}
      <EmptyState icon="folder" title="No project selected" />
    {:else if slot?.error && !slot.data}
      <ErrorState error={slot.error} onretry={() => tools.load(projectId)} />
    {:else if !slot?.data}
      <Spinner />
    {:else if shown.length === 0}
      <EmptyState
        icon="wrench"
        title="No tools yet"
        body="Install a plugin that adds tools, or add one in Settings under Tools."
      >
        {#snippet actions()}
          <Button
            onclick={() => {
              onclose();
              void dispatch('settings.open', { section: 'tools' });
            }}>Open tool settings</Button
          >
        {/snippet}
      </EmptyState>
    {:else}
      <ul>
        {#each shown as t (t.id)}
          <li>
            <button class="row" onclick={() => open(t)}>
              <Icon name={t.icon ?? 'wrench'} />
              <span class="label">{t.label}</span>
              {#if t.description}<span class="desc">{t.description}</span>{/if}
              {#if t.source.kind === 'plugin'}<Badge>{t.source.plugin_id}</Badge>{/if}
            </button>
            {#if !installed(t)}
              <div class="missing" role="status">
                <span>Not installed. {checks[t.id]?.install_hint ?? ''}</span>
                <Button size="sm" onclick={() => check(t)}>Check again</Button>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</Sheet>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 100%;
    min-height: var(--k-row-height);
    padding: var(--k-space-2) var(--k-space-3);
    border: 0;
    border-radius: var(--k-radius);
    background: none;
    color: var(--k-fg);
    text-align: left;
    cursor: pointer;
  }

  .row:hover,
  .row:focus-visible {
    background: var(--k-bg-hover);
  }

  .label {
    font-weight: 600;
  }

  .desc {
    flex: 1;
    color: var(--k-fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .missing {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--k-space-3);
    padding: 0 var(--k-space-3) var(--k-space-2) 36px;
    color: var(--k-warn);
    font-size: var(--k-font-size-sm);
  }
</style>
