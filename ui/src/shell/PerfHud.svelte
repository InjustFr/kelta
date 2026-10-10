<script lang="ts">
  import type { PerfSnapshot } from '$lib/gen';
  import { perfSnapshot } from '$lib/ipc/commands';
  import { terminalPool } from '$lib/terminal';
  import { Button, IconButton, Spinner } from '$lib/ui';

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let snap = $state<PerfSnapshot | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  // On demand only: one snapshot when the HUD opens, another on "Refresh". No polling.
  async function refresh(): Promise<void> {
    loading = true;
    try {
      snap = await perfSnapshot();
      error = null;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void refresh();
  });

  const mb = (kb: number): string => `${(kb / 1024).toFixed(kb >= 10240 ? 0 : 1)} MB`;
  const kelta = $derived(
    snap
      ? snap.processes.filter((p) => p.role !== 'child').reduce((a, p) => a + p.pss_or_footprint_kb, 0)
      : 0,
  );
  const children = $derived(
    snap
      ? snap.processes.filter((p) => p.role === 'child').reduce((a, p) => a + p.pss_or_footprint_kb, 0)
      : 0,
  );
</script>

<div class="hud" role="dialog" aria-label="Performance" data-testid="perf-hud">
  <header>
    <strong>Performance</strong>
    <span class="spacer"></span>
    <Button size="sm" icon="refresh-cw" onclick={refresh} {loading}>Refresh</Button>
    <IconButton icon="x" label="Close" size="sm" onclick={onclose} />
  </header>
  {#if error}
    <p class="err">{error}</p>
  {:else if !snap}
    <div class="wait"><Spinner size={14} /></div>
  {:else}
    <dl>
      <dt>Kelta (core + web view)</dt>
      <dd data-testid="perf-kelta">{mb(kelta)}</dd>
      <dt>Child processes</dt>
      <dd data-testid="perf-children">{mb(children)}</dd>
      <dt>Live terminal views</dt>
      <dd>{terminalPool.liveCount} (core reports {snap.live_views})</dd>
      <dt>Armed timers</dt>
      <dd>{snap.timers_armed}</dd>
      <dt>HTTP server</dt>
      <dd>{snap.http_server ? 'running' : 'stopped'}</dd>
    </dl>
    <table>
      <tbody>
        {#each snap.processes as p (p.pid)}
          <tr
            ><td>{p.name}</td><td class="role">{p.role}</td><td class="num">{mb(p.pss_or_footprint_kb)}</td
            ></tr
          >
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .hud {
    position: absolute;
    right: var(--k-space-3);
    bottom: calc(var(--k-statusbar-height) + 4px);
    z-index: var(--k-z-menu);
    width: 340px;
    max-height: 60vh;
    overflow: auto;
    padding: var(--k-space-3) var(--k-space-4);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
    font-size: var(--k-font-size-sm);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin-bottom: var(--k-space-3);
  }

  .spacer {
    flex: 1;
  }

  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: var(--k-space-1) var(--k-space-4);
    margin: 0 0 var(--k-space-3);
  }

  dt {
    color: var(--k-fg-muted);
  }

  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    color: var(--k-fg-muted);
  }

  td {
    padding: 1px 0;
  }

  .role {
    padding: 0 var(--k-space-3);
    color: var(--k-fg-subtle);
  }

  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .err {
    color: var(--k-danger);
  }

  .wait {
    display: flex;
    justify-content: center;
    padding: var(--k-space-4);
  }
</style>
