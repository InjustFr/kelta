<script lang="ts">
  // Slim banner above the workspace while an installed update waits for a restart (#74). Not a toast:
  // it stays until Restart, or Later for this session. Kelta never restarts by itself.
  import { t } from '$lib/i18n/index.svelte';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  import { dispatch } from '$lib/actions';
  import { Button } from '$lib/ui';

  import { updates } from './updates.svelte';

  onMount(() => {
    const refresh = (): void => void updates.refresh().catch(() => {});
    refresh();
    window.addEventListener('focus', refresh);
    // The background check finishes after the first read: the backend says when (no Tauri in the mock).
    const unlisten = listen('kelta://update', refresh).catch(() => () => {});
    return () => {
      window.removeEventListener('focus', refresh);
      void unlisten.then((off) => off());
    };
  });
</script>

{#if updates.banner}
  <div class="banner" role="status" data-testid="update-banner">
    <span class="text"
      >{t('Kelta {version} is installed. Restart to use it.', { version: updates.banner })}</span
    >
    <Button size="sm" variant="primary" onclick={() => dispatch('app.restart')}>{t('Restart')}</Button>
    <Button size="sm" variant="ghost" onclick={() => (updates.dismissed = updates.banner)}
      >{t('Later')}</Button
    >
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
