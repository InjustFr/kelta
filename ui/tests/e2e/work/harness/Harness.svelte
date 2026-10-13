<script lang="ts">
  import { paneRegistry, sheetRegistry, tabHeaderRegistry, type RegisteredSheetKey } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { activeTab, allPanes } from '$lib/layout';
  import { layout, projects, toasts, ui } from '$lib/stores';
  import Toast from '$lib/ui/Toast.svelte';

  import GlobalHost from '../../../../src/shell/GlobalHost.svelte';

  const projectId = $derived(projects.activeId ?? 'shop');
  const current = $derived(layout.get(projectId));
  const tab = $derived(current ? activeTab(current) : null);
  const panes = $derived(tab ? allPanes(tab.root) : []);
  const sheet = $derived(ui.sheet);
  const sheetLoader = $derived(sheet ? sheetRegistry[sheet.key as RegisteredSheetKey] : undefined);
  const headerWorkId = $derived(tab?.work_item_id ?? null);
</script>

<main class="host" data-testid="harness">
  {#if tab && headerWorkId}
    {#await tabHeaderRegistry.work() then mod}
      {@const Header = mod.default}
      <div class="bar"><Header {projectId} tabId={tab.id} workItemId={headerWorkId} /></div>
    {/await}
  {/if}
  <div class="panes">
    {#if ui.view}
      <GlobalHost view={ui.view} />
    {:else}
      {#each panes as p (p.id)}
        {#if p.content.kind !== 'empty'}
          <div class="pane" data-pane={p.content.kind}>
            {#await paneRegistry[p.content.kind]() then mod}
              {@const Pane = mod.default}
              <Pane
                {projectId}
                tabId={tab?.id ?? ''}
                paneId={p.id}
                content={p.content}
                visible={true}
                focused={tab?.focused_pane === p.id}
              />
            {/await}
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</main>

{#if sheet && sheetLoader}
  {#key sheet}
    {#await sheetLoader() then mod}
      {@const Sheet = mod.default}
      <Sheet {...sheet.props} onclose={() => ui.closeSheet(sheet.key)} />
    {/await}
  {/key}
{/if}

<div class="toasts" data-testid="toasts">
  {#each toasts.list as entry (entry.id)}
    <Toast
      toast={entry.toast}
      ondismiss={() => toasts.dismiss(entry.id)}
      onaction={(a) => void dispatch(a.command, (a.args ?? undefined) as Record<string, unknown> | undefined)}
    />
  {/each}
</div>

<style>
  .host {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  /* Stands in for the shell's context bar, which hosts the work segment. */
  .bar {
    display: flex;
    height: 30px;
    padding: 0 8px;
  }

  .panes {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    border-right: 1px solid var(--k-border);
  }

  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: var(--k-z-toast);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
</style>
