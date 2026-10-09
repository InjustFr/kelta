<script lang="ts">
  // Live memory per component and per session, on demand (a Refresh button; nothing polls).
  import type { SettingsSectionProps } from '$app/registry';
  import type { PerfSnapshot } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import Button from '$lib/ui/Button.svelte';

  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  let snap = $state<PerfSnapshot | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

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
  {#snippet before()}
    <section class="perf" data-testid="perf">
      <header>
        <h3>Live memory</h3>
        <Button size="sm" icon="refresh-cw" {loading} onclick={refresh}>Refresh</Button>
      </header>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if snap}
        <table>
          <thead><tr><th>Process</th><th>Role</th><th class="num">Memory</th></tr></thead>
          <tbody>
            {#each snap.processes as p (p.pid)}
              <tr
                ><td>{p.name} <span class="muted">({p.pid})</span></td><td>{p.role}</td><td class="num"
                  >{mb(p.pss_or_footprint_kb)}</td
                ></tr
              >
            {/each}
            <tr class="sum"
              ><td colspan="2">Kelta total (core + web view)</td><td class="num">{mb(total)}</td></tr
            >
          </tbody>
        </table>
        {#if snap.sessions.length > 0}
          <h4>Sessions</h4>
          <table>
            <thead
              ><tr
                ><th>Session</th><th class="num">History lines</th><th class="num">Model</th><th class="num"
                  >Child process</th
                ></tr
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
          Live terminal views: {snap.live_views} · armed timers: {snap.timers_armed} · local server:
          {snap.http_server ? 'running' : 'stopped'}. Plugin screens are destroyed when hidden unless kept
          alive below.
        </p>
      {/if}
    </section>
  {/snippet}
</SectionForm>

<style>
  .perf {
    margin-bottom: var(--k-space-5);
    padding: var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  h3,
  h4 {
    margin: 0 0 var(--k-space-3);
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
