<script lang="ts">
  // Settings → Triggers: list with origin, test with a sample payload, and the recent run log.
  import { t } from '$lib/i18n/index.svelte';
  import { onMount } from 'svelte';

  import type { SettingsSectionProps } from '$app/registry';
  import type { JsonValue, TriggerInfo, TriggerRun } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { plugins, toasts, ui } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  import SettingsCard from '../../settings/layout/SettingsCard.svelte';
  import SettingsGroup from '../../settings/layout/SettingsGroup.svelte';
  import SectionForm from '../../settings/SectionForm.svelte';

  let props: SettingsSectionProps = $props();
  const layer = $derived(props.layer);

  let log = $state<TriggerRun[]>([]);
  let testing = $state<TriggerInfo | null>(null);
  let payload = $state('{}');
  let payloadError = $state<string | null>(null);
  let result = $state<TriggerRun | null>(null);

  $effect(() => {
    if (plugins.triggers.fetchedAt === null && !plugins.triggers.loading && !plugins.triggers.error)
      void plugins.loadTriggers();
  });
  onMount(() => void loadLog());

  async function loadLog(): Promise<void> {
    try {
      log = await ipc.triggerLog({ limit: 50 });
    } catch (e) {
      toasts.error(e, t('Trigger log'));
    }
  }

  function origin(trig: TriggerInfo): string {
    const o = trig.origin;
    if (o.kind === 'plugin') return t('plugin {id}', { id: o.plugin_id });
    if (o.kind === 'repo')
      return o.trusted ? t('repo {id}', { id: o.repo_id }) : t('repo {id} (untrusted)', { id: o.repo_id });
    return o.kind === 'project' ? t('project {id}', { id: o.project_id }) : t('global');
  }

  async function runTest(): Promise<void> {
    if (!testing) return;
    let parsed: JsonValue;
    try {
      parsed = JSON.parse(payload) as JsonValue;
      payloadError = null;
    } catch (e) {
      payloadError = e instanceof Error ? e.message : t('invalid JSON');
      return;
    }
    try {
      result = await ipc.triggerTest({ trigger_id: testing.id, payload: parsed });
      void loadLog();
    } catch (e) {
      toasts.error(e, t('Test {id}', { id: testing.id }));
    }
  }
</script>

<SectionForm sectionId="triggers" {...props}>
  {#snippet after()}
    <SettingsGroup heading={t('All triggers')} data-layer={layer}>
      {#if plugins.triggers.error && !plugins.triggers.data}
        <ErrorState error={plugins.triggers.error} onretry={() => plugins.loadTriggers()} />
      {:else if !plugins.triggers.data}
        <Spinner />
      {:else if plugins.triggers.data.length === 0}
        <EmptyState
          icon="zap"
          title={t('No triggers yet')}
          body={t(
            'Triggers run a command when something happens, such as a session finishing. Install a plugin that adds some.',
          )}
        >
          {#snippet actions()}
            <Button size="sm" onclick={() => ui.openSheet('plugin_install', {})}>{t('Install plugin')}</Button
            >
          {/snippet}
        </EmptyState>
      {:else}
        {#each plugins.triggers.data as trig (trig.id)}
          <SettingsCard heading={trig.id} mono dim={!trig.enabled} data-trigger={trig.id}>
            {#snippet head()}
              <code>{trig.on}</code>
              <Badge>{origin(trig)}</Badge>
              {#if !trig.enabled}<Badge tone="warn">{t('disabled')}</Badge>{/if}
              {#if trig.description}<span class="muted">{trig.description}</span>{/if}
            {/snippet}
            {#snippet actions()}
              <Button size="sm" variant="ghost" onclick={() => ((testing = trig), (result = null))}
                >{t('Test')}</Button
              >
            {/snippet}
            {#if testing?.id === trig.id}
              <div class="test">
                <TextInput
                  bind:value={payload}
                  multiline
                  rows={5}
                  label={t('Sample event payload (JSON)')}
                  error={payloadError}
                />
                <div>
                  <Button variant="primary" onclick={runTest}>{t('Run test')}</Button>
                </div>
                {#if result}
                  <p class={result.ok ? 'ok' : 'bad'}>
                    {result.ok ? t('OK') : t('Failed')}
                    {result.detail ?? ''}
                  </p>
                {/if}
              </div>
            {/if}
          </SettingsCard>
        {/each}
      {/if}
    </SettingsGroup>

    <SettingsGroup heading={t('Recent runs')}>
      {#snippet actions()}
        <Button size="sm" variant="ghost" icon="refresh-cw" onclick={loadLog}>{t('Refresh')}</Button>
      {/snippet}
      {#if log.length === 0}
        <p class="muted">{t('No trigger has run yet.')}</p>
      {:else}
        <table>
          <tbody>
            {#each log as r, i (i)}
              <tr>
                <td class="muted">{r.ts}</td>
                <td>{r.trigger_id}</td>
                <td><code>{r.event}</code></td>
                <td class={r.ok ? 'ok' : 'bad'}>{r.ok ? t('ok') : t('failed')}</td>
                <td class="muted">{r.detail ?? ''}{r.depth ? `, ${t('depth {n}', { n: r.depth })}` : ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </SettingsGroup>
  {/snippet}
</SectionForm>

<style>
  p {
    margin: 0;
  }

  .test {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    padding: var(--k-space-2) 0;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--k-font-size-sm);
  }

  td {
    padding: var(--k-space-2) var(--k-space-3) var(--k-space-2) 0;
    border-bottom: 1px solid var(--k-border);
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .ok {
    color: var(--k-ok);
  }

  .bad {
    color: var(--k-danger);
  }
</style>
