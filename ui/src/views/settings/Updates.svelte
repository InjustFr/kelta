<script lang="ts">
  // Settings → General › Updates (global layer, drawn inside that copy group): running version and channel, the last check, Check now and
  // What's new (#74). Check failures show here only, never as a toast.
  import { t } from '$lib/i18n/index.svelte';
  import { onMount } from 'svelte';

  import { openExternal } from '$lib/ipc/commands';
  import { Button } from '$lib/ui';

  import { releasePage, restartApp, updates } from '../../shell/updates.svelte';
  import { clock } from '../../shell/usage';
  import SettingsRow from './layout/SettingsRow.svelte';

  onMount(() => void updates.refresh().catch(() => {}));

  const s = $derived(updates.status);
  const at = (iso: string | null): string => (iso ? clock(Date.parse(iso) / 1000) : '');

  const line = $derived.by(() => {
    if (!s) return '';
    const st = s.state;
    switch (st.type) {
      case 'idle':
        return s.checked_at ? t('Last checked {time}', { time: at(s.checked_at) }) : t('Not checked yet');
      case 'checking':
        return t('Checking…');
      case 'downloading':
        return t('Downloading…');
      case 'ready':
        return t('Restart to use {version}', { version: st.version });
      case 'up_to_date':
        return t('Last checked {time}, up to date', { time: at(s.checked_at) });
      case 'failed':
        return t('Last check failed: {message}', { message: st.message });
      case 'unsupported':
        return st.reason;
    }
  });
</script>

{#if s}
  <SettingsRow label="Kelta {s.current} ({s.channel})" actions data-testid="updates">
    {#snippet control()}
      {#if s.state.type === 'ready'}
        <Button variant="primary" onclick={() => void restartApp()}>{t('Restart')}</Button>
      {/if}
      {#if s.state.type !== 'unsupported'}
        <Button
          loading={updates.checking}
          onclick={() => void updates.check().catch(() => {})}
          data-testid="update-check">{t('Check now')}</Button
        >
      {/if}
      <Button
        variant="ghost"
        onclick={() => void openExternal({ url: releasePage(s.channel) }).catch(() => {})}
        >{t("What's new")}</Button
      >
    {/snippet}
    <p class:failed={s.state.type === 'failed'} data-testid="update-state">{line}</p>
  </SettingsRow>
{/if}

<style>
  .failed {
    color: var(--k-warn);
  }
</style>
