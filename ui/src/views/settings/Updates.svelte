<script lang="ts">
  // Settings → General (global layer): running version and channel, the last check, Check now and
  // What's new (#74). Check failures show here only, never as a toast.
  import { onMount } from 'svelte';

  import { openExternal } from '$lib/ipc/commands';
  import { Button } from '$lib/ui';

  import { releasePage, restartApp, updates } from '../../shell/updates.svelte';
  import { clock } from '../../shell/usage';
  import SettingsGroup from './layout/SettingsGroup.svelte';
  import SettingsRow from './layout/SettingsRow.svelte';

  onMount(() => void updates.refresh().catch(() => {}));

  const s = $derived(updates.status);
  const at = (iso: string | null): string => (iso ? clock(Date.parse(iso) / 1000) : '');

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
  <SettingsGroup heading="Updates" data-testid="updates">
    <SettingsRow label="Kelta {s.current} ({s.channel})" actions>
      {#snippet control()}
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
          onclick={() => void openExternal({ url: releasePage(s.channel) }).catch(() => {})}
          >What's new</Button
        >
      {/snippet}
      <p class:failed={s.state.type === 'failed'} data-testid="update-state">{line}</p>
    </SettingsRow>
  </SettingsGroup>
{/if}

<style>
  .failed {
    color: var(--k-warn);
  }
</style>
