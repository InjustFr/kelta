<script lang="ts">
  import { sheetRegistry, type AnyComponent, type RegisteredSheetKey } from '$app/registry';
  import { ui } from '$lib/stores';
  import type { SheetEntry } from '$lib/stores/ui.svelte';
  import { ErrorState } from '$lib/ui';

  import { lazyComponents } from './lazy.svelte';

  function loaderFor(key: SheetEntry['key']): (() => Promise<{ default: AnyComponent }>) | null {
    if (key === 'session_new') return () => import('./NewSessionSheet.svelte');
    return sheetRegistry[key as RegisteredSheetKey] ?? null;
  }

  $effect(() => {
    for (const entry of ui.sheets) {
      const loader = loaderFor(entry.key);
      if (loader) lazyComponents.ensure(`sheet:${entry.key}`, loader);
    }
  });
</script>

{#each ui.sheets as entry, i (i)}
  {@const loaded = lazyComponents.get(`sheet:${entry.key}`)}
  {@const Sheet = loaded.component as AnyComponent | null}
  {#if Sheet}
    <svelte:boundary onerror={(e) => console.error(`[kelta] sheet ${entry.key} crashed`, e)}>
      <Sheet {...entry.props} onclose={() => ui.closeSheet(entry.key)} />
      {#snippet failed(error)}
        <div class="failed">
          <ErrorState
            error={error instanceof Error ? error : String(error)}
            title="This sheet crashed"
            onretry={() => ui.closeSheet(entry.key)}
          />
        </div>
      {/snippet}
    </svelte:boundary>
  {:else if loaded.error}
    <div class="failed">
      <ErrorState
        error={loaded.error}
        title="Could not load this sheet"
        onretry={() => ui.closeSheet(entry.key)}
      />
    </div>
  {/if}
{/each}

<style>
  .failed {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: 420px;
    z-index: var(--k-z-sheet);
    background: var(--k-bg-elev);
    border-left: 1px solid var(--k-border);
  }
</style>
