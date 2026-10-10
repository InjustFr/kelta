<script lang="ts">
  // Settings → General (global layer): running version and channel, the last check, Check now and
  // What's new (#74). Check failures show here only, never as a toast.
  import { onMount } from 'svelte';

  import { openExternal } from '$lib/ipc/commands';
  import { Button } from '$lib/ui';

  import { releasePage, restartApp, updates } from '../../shell/updates.svelte';

  onMount(() => void updates.refresh().catch(() => {}));

  const s = $derived(updates.status);
  const at = (iso: string | null): string =>
    iso ? new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';

  const line = $derived.by(() => {
    if (!s) return '';
    const st = s.state;
    switch (st.type) {
      case 'idle':
        return s.checked_at ? `Last checked ${at(s.checked_at)}` : 'Not checked yet';
      case 'checking':
        return 'Checking…';
      case 'downloading':
        return 'Downloading…';
      case 'ready':
        return `Restart to use ${st.version}`;
      case 'up_to_date':
        return `Last checked ${at(s.checked_at)}, up to date`;
      case 'failed':
        return `Last check failed: ${st.message}`;
      case 'unsupported':
        return st.reason;
    }
  });
</script>

{#if s}
  <div class="updates" data-testid="updates">
    <h3>Updates</h3>
    <p class="version">Kelta {s.current} <span class="channel">({s.channel})</span></p>
    <p class="state" class:failed={s.state.type === 'failed'} data-testid="update-state">{line}</p>
    <div class="actions">
      {#if s.state.type === 'ready'}
        <Button variant="primary" onclick={() => void restartApp()}>Restart</Button>
      {/if}
      {#if s.state.type !== 'unsupported'}
        <Button
          loading={updates.checking}
          onclick={() => void updates.check().catch(() => {})}
          data-testid="update-check">Check now</Button
        >
      {/if}
      <Button
        variant="ghost"
        onclick={() => void openExternal({ url: releasePage(s.channel) }).catch(() => {})}>What's new</Button
      >
    </div>
  </div>
{/if}

<style>
  .updates {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin-bottom: var(--k-space-4);
  }

  h3 {
    margin: 0;
    font-size: var(--k-font-size);
  }

  p {
    margin: 0;
  }

  .channel,
  .state {
    color: var(--k-fg-muted);
  }

  .state.failed {
    color: var(--k-warn);
  }

  .actions {
    display: flex;
    gap: var(--k-space-2);
  }
</style>
