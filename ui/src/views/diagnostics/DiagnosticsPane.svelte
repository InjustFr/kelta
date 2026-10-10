<script lang="ts">
  // Diagnostics: dependency and environment checks (claude, nvim, git, gh, glab, secret backend,
  // notification daemon, hooks) with a fix per failing row. Runs on open and on demand only.
  import type { PaneProps } from '$app/registry';
  import type { AppInfo, Diagnostics } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { toasts } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  import { counts, reportText, sortChecks, STATUS_ICON, STATUS_TONE } from './report';

  let { content }: PaneProps<'diagnostics'> = $props();

  let info = $state<AppInfo | null>(null);
  let diag = $state<Diagnostics | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function run(): Promise<void> {
    loading = true;
    error = null;
    try {
      [info, diag] = await Promise.all([ipc.appInfo(), ipc.diagnosticsRun()]);
    } catch (err) {
      error = toIpcError('diagnostics_run', err).message;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void run();
  });

  async function copy(): Promise<void> {
    try {
      await ipc.clipboardWrite({ kind: 'clipboard', text: reportText(info, diag) });
      toasts.info('Report copied');
    } catch (err) {
      toasts.error(err, 'Could not copy the report');
    }
  }

  const sorted = $derived(diag ? sortChecks(diag.checks) : []);
  const summary = $derived(diag ? counts(diag.checks) : null);
</script>

<div class="diagnostics" data-pane-kind={content.kind}>
  <header>
    <h2>Diagnostics</h2>
    <div class="spacer"></div>
    <Button size="sm" icon="copy" onclick={copy} disabled={!diag}>Copy report</Button>
    <Button size="sm" icon="refresh-cw" {loading} onclick={run}>Check again</Button>
  </header>

  {#if error}
    <ErrorState title="Diagnostics failed" {error} onretry={run} />
  {:else if !diag}
    <div class="loading"><Spinner size={16} /> Checking your environment…</div>
  {:else}
    {#if summary}
      <p class="summary" data-testid="diagnostics-summary">
        {#if summary.fail === 0 && summary.warn === 0}
          Everything looks good.
        {:else}
          {summary.fail} failing, {summary.warn} warning{summary.warn === 1 ? '' : 's'}.
        {/if}
      </p>
    {/if}
    <ul class="checks">
      {#each sorted as c (c.id)}
        <li data-testid="check" data-check-id={c.id} data-status={c.status}>
          <span class="icon {c.status}"><Icon name={STATUS_ICON[c.status]} size={16} /></span>
          <div class="body">
            <div class="row">
              <strong>{c.label}</strong>
              <Badge tone={STATUS_TONE[c.status]}>{c.status}</Badge>
            </div>
            <p class="detail">{c.detail}</p>
            {#if c.fix}<p class="fix"><code>{c.fix}</code></p>{/if}
          </div>
        </li>
      {/each}
    </ul>

    {#if info}
      <section class="info" data-testid="app-info">
        <h3>Environment</h3>
        <dl>
          <dt>Version</dt>
          <dd>{info.version}&ensp;<code>{info.platform}/{info.arch}</code></dd>
          <dt>Config</dt>
          <dd><code>{info.config_dir}</code></dd>
          <dt>Data</dt>
          <dd><code>{info.data_dir}</code></dd>
          <dt>Runtime</dt>
          <dd><code>{info.runtime_dir}</code></dd>
          {#if info.claude}
            <dt>Claude</dt>
            <dd>{info.claude.version}&ensp;<code>{info.claude.path}</code></dd>
          {/if}
          {#if info.safe_graphics}
            <dt>Graphics</dt>
            <dd>Safe mode for this launch</dd>
          {/if}
        </dl>
      </section>
    {/if}
  {/if}
</div>

<style>
  .diagnostics {
    height: 100%;
    overflow: auto;
    padding: var(--k-space-4) var(--k-space-5);
    max-width: 860px;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  h2,
  h3 {
    margin: 0;
  }

  h3 {
    margin-bottom: var(--k-space-3);
  }

  .spacer {
    flex: 1;
  }

  .loading {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-5);
    color: var(--k-fg-muted);
  }

  .summary {
    color: var(--k-fg-muted);
  }

  .checks {
    list-style: none;
    margin: 0 0 var(--k-space-5);
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  li {
    display: flex;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .icon.ok {
    color: var(--k-ok);
  }

  .icon.warn {
    color: var(--k-warn);
  }

  .icon.fail {
    color: var(--k-danger);
  }

  .body {
    flex: 1;
    min-width: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .detail,
  .fix {
    margin: var(--k-space-1) 0 0;
    color: var(--k-fg-muted);
    overflow-wrap: anywhere;
  }

  code {
    font-family: var(--k-font-mono);
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--k-space-2) var(--k-space-4);
    margin: 0;
  }

  dt {
    color: var(--k-fg-muted);
  }

  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
