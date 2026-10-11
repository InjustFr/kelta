<script lang="ts">
  // Live memory per component and per session, on demand (a Refresh button; nothing polls).
  import { t } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';
  import type { PerfSnapshot } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import Button from '$lib/ui/Button.svelte';

  import PerfHud from '../../../shell/PerfHud.svelte';

  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SettingsRow from '../layout/SettingsRow.svelte';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  let snap = $state<PerfSnapshot | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let hud = $state(false);

  async function refresh(): Promise<void> {
    loading = true;
    error = null;
    try {
      snap = await ipc.perfSnapshot();
    } catch (err) {
      error = toIpcError('perf_snapshot', err).message;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void refresh();
  });

  const mb = (kb: number | null | undefined): string => (kb == null ? '–' : `${(kb / 1024).toFixed(1)} MB`);
  const total = $derived(snap ? snap.processes.reduce((a, p) => a + (p.pss_or_footprint_kb ?? 0), 0) : 0);
</script>

<SectionForm sectionId="performance" {...props}>
  {#snippet after()}
    <SettingsRow label={t('Performance overlay')} actions>
      {#snippet control()}
        <Button size="sm" icon="cpu" onclick={() => (hud = !hud)}>{t('Show performance overlay')}</Button>
      {/snippet}
    </SettingsRow>
    {#if hud}<PerfHud onclose={() => (hud = false)} />{/if}
    <SettingsGroup heading={t('Live memory')} data-testid="perf">
      {#snippet actions()}
        <Button size="sm" variant="ghost" icon="refresh-cw" {loading} onclick={refresh}>{t('Refresh')}</Button
        >
      {/snippet}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if snap}
        <table>
          <thead><tr><th>{t('Process')}</th><th>{t('Role')}</th><th class="num">{t('Memory')}</th></tr></thead
          >
          <tbody>
            {#each snap.processes as p (p.pid)}
              <tr
                ><td>{p.name} <span class="muted">({p.pid})</span></td><td>{p.role}</td><td class="num"
                  >{mb(p.pss_or_footprint_kb)}</td
                ></tr
              >
            {/each}
            <tr class="sum"
              ><td colspan="2">{t('Kelta total (core + web view)')}</td><td class="num">{mb(total)}</td></tr
            >
          </tbody>
        </table>
        {#if snap.sessions.length > 0}
          <h4>{t('Sessions')}</h4>
          <table>
            <thead
              ><tr
                ><th>{t('Session')}</th><th class="num">{t('History lines')}</th><th class="num"
                  >{t('Model')}</th
                ><th class="num">{t('Child process')}</th></tr
              ></thead
            >
            <tbody>
              {#each snap.sessions as s (s.id)}
                <tr>
                  <td>{s.name}</td>
                  <td class="num">{s.history_lines}</td>
                  <td class="num">{mb(s.model_bytes / 1024)}</td>
                  <td class="num">{mb(s.child_kb)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
        <p class="muted">
          {t(
            'Live terminal views: {views}. Armed timers: {timers}. Local server: {server}. Plugin screens are destroyed when hidden unless kept alive below.',
            {
              views: snap.live_views,
              timers: snap.timers_armed,
              server: snap.http_server ? t('running') : t('stopped'),
            },
          )}
        </p>
      {/if}
    </SettingsGroup>
  {/snippet}
</SectionForm>

<style>
  h4 {
    margin: 0 0 var(--k-space-2);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: var(--k-space-4);
  }

  th,
  td {
    text-align: left;
    padding: var(--k-space-2) var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
  }

  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .sum td {
    font-weight: 600;
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .error {
    color: var(--k-danger);
  }
</style>
