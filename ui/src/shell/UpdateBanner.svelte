<script lang="ts">
  // Slim banner above the workspace while an installed update waits for a restart (#74). Not a toast:
  // it stays until Restart, or Later for this session. Kelta never restarts by itself.
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import { Button } from '$lib/ui';

  import { updates } from './updates.svelte';

  onMount(() => {
    // shortcut: read on start and on focus only, so an update a focus check installs shows on the
    // next focus; push a UiEvent if that lag matters.
    const refresh = (): void => void updates.refresh().catch(() => {});
    refresh();
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  });
</script>

{#if updates.banner}
  <div class="banner" role="status" data-testid="update-banner">
    <span class="text">Kelta {updates.banner} is installed. Restart to use it.</span>
    <Button size="sm" variant="primary" onclick={() => dispatch('app.restart')}>Restart</Button>
    <Button size="sm" variant="ghost" onclick={() => (updates.dismissed = updates.banner)}>Later</Button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    flex: none;
    padding: var(--k-space-1) var(--k-space-3);
    background: var(--k-bezel-raised);
    border-bottom: 1px solid var(--k-accent);
    font-size: var(--k-font-size-xs);
  }

  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
