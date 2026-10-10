<script lang="ts">
  // The rail's global views (Tickets, Now, Reviews): one pane rendered outside any project, with
  // the `GLOBAL` sentinel as its project (SPEC §1).
  import { paneComponent, type AnyComponent } from '$app/registry';
  import { GLOBAL, type GlobalView } from '$lib/stores';
  import { ErrorState, Spinner } from '$lib/ui';

  import { globalContent } from '../views/work/nav';
  import { lazyComponents } from './lazy.svelte';
  import { VIEW_NAMES } from './nav';

  let { view }: { view: GlobalView } = $props();

  const content = $derived(globalContent(view));
  const key = $derived(`pane:${content.kind}`);
  const entry = $derived(lazyComponents.get(key));
  const Component = $derived(entry.component as AnyComponent | null);
  const name = $derived(VIEW_NAMES[view]);

  $effect(() => {
    const loader = paneComponent(content.kind);
    if (loader) lazyComponents.ensure(key, loader);
  });
</script>

<div class="global" data-testid="global-host" data-view={view}>
  <svelte:boundary onerror={(e) => console.error(`[kelta] ${name} crashed`, e)}>
    {#if Component}
      {#key view}
        <Component projectId={GLOBAL} tabId={GLOBAL} paneId={view} {content} visible={true} focused={true} />
      {/key}
    {:else if entry.error}
      <ErrorState error={entry.error} title={`Could not load ${name}`} />
    {:else}
      <div class="loading"><Spinner size={16} /></div>
    {/if}
    {#snippet failed(error)}
      <ErrorState error={error instanceof Error ? error : String(error)} title={`${name} crashed`} />
    {/snippet}
  </svelte:boundary>
</div>

<style>
  .global {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: var(--k-bg);
  }

  .loading {
    display: flex;
    justify-content: center;
    padding: var(--k-space-6);
  }
</style>
