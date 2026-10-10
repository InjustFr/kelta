<script lang="ts">
  import { paneComponent, type AnyComponent } from '$app/registry';
  import { ErrorState, Spinner } from '$lib/ui';

  import { lazyComponents } from './lazy.svelte';

  const loader = paneComponent('inbox');
  const entry = $derived(lazyComponents.get('pane:inbox'));
  const Component = $derived(entry.component as AnyComponent | null);

  $effect(() => {
    if (loader) lazyComponents.ensure('pane:inbox', loader);
  });
</script>

<div class="inbox" data-testid="inbox-host">
  <svelte:boundary onerror={(e) => console.error('[kelta] Now crashed', e)}>
    {#if Component}
      <Component
        projectId="inbox"
        tabId="inbox"
        paneId="inbox"
        content={{ kind: 'inbox' }}
        visible={true}
        focused={true}
      />
    {:else if entry.error}
      <ErrorState error={entry.error} title="Could not load Now" />
    {:else}
      <div class="loading"><Spinner size={16} /></div>
    {/if}
    {#snippet failed(error)}
      <ErrorState error={error instanceof Error ? error : String(error)} title="Now crashed" />
    {/snippet}
  </svelte:boundary>
</div>

<style>
  .inbox {
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
