<script lang="ts">
  // Settings → Triggers: list with origin, test with a sample payload, and the recent run log.
  import { onMount } from 'svelte';

  import type { SettingsSectionProps } from '$app/registry';
  import type { JsonValue, TriggerInfo, TriggerRun } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { plugins, toasts } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  let { layer }: SettingsSectionProps = $props();

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
      toasts.error(e, 'Trigger log');
    }
  }

  function origin(t: TriggerInfo): string {
    const o = t.origin;
    if (o.kind === 'plugin') return `plugin ${o.plugin_id}`;
    if (o.kind === 'repo') return `repo ${o.repo_id}${o.trusted ? '' : ' (untrusted)'}`;
    return o.kind === 'project' ? `project ${o.project_id}` : 'global';
  }

  async function runTest(): Promise<void> {
    if (!testing) return;
    let parsed: JsonValue;
    try {
      parsed = JSON.parse(payload) as JsonValue;
      payloadError = null;
    } catch (e) {
      payloadError = e instanceof Error ? e.message : 'invalid JSON';
      return;
    }
    try {
      result = await ipc.triggerTest({ trigger_id: testing.id, payload: parsed });
      void loadLog();
    } catch (e) {
      toasts.error(e, `Test ${testing.id}`);
    }
  }
</script>

<section data-layer={layer} class="triggers">
  <h2>Triggers</h2>
  <p class="muted">Defined as <code>[[triggers]]</code> in config files, or contributed by plugins.</p>

  {#if plugins.triggers.error && !plugins.triggers.data}
    <ErrorState error={plugins.triggers.error} onretry={() => plugins.loadTriggers()} />
  {:else if !plugins.triggers.data}
    <Spinner />
  {:else if plugins.triggers.data.length === 0}
    <EmptyState icon="zap" title="No triggers configured" />
  {:else}
    <ul>
      {#each plugins.triggers.data as t (t.id)}
        <li>
          <strong>{t.id}</strong>
          <code>{t.on}</code>
          <Badge>{origin(t)}</Badge>
          {#if !t.enabled}<Badge tone="warn">disabled</Badge>{/if}
          <span class="grow muted">{t.description ?? ''}</span>
          <Button size="sm" variant="ghost" onclick={() => ((testing = t), (result = null))}>Test</Button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if testing}
    <div class="test">
      <h3>Test {testing.id}</h3>
      <TextInput
        bind:value={payload}
        multiline
        rows={5}
        label="Sample event payload (JSON)"
        error={payloadError}
      />
      <Button variant="primary" onclick={runTest}>Run test</Button>
      {#if result}
        <p class={result.ok ? 'ok' : 'bad'}>{result.ok ? 'OK' : 'Failed'} {result.detail ?? ''}</p>
      {/if}
    </div>
  {/if}

  <div class="loghead">
    <h3>Recent runs</h3>
    <Button size="sm" variant="ghost" icon="refresh-cw" onclick={loadLog}>Refresh</Button>
  </div>
  {#if log.length === 0}
    <p class="muted">No trigger has run yet.</p>
  {:else}
    <table>
      <tbody>
        {#each log as r, i (i)}
          <tr>
            <td class="muted">{r.ts}</td>
            <td>{r.trigger_id}</td>
            <td><code>{r.event}</code></td>
            <td class={r.ok ? 'ok' : 'bad'}>{r.ok ? 'ok' : 'failed'}</td>
            <td class="muted">{r.detail ?? ''}{r.depth ? ` · depth ${r.depth}` : ''}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  h2,
  h3 {
    margin: 0;
  }

  h2 {
    font-size: var(--k-font-size-lg);
  }

  ul {
    list-style: none;
    margin: var(--k-space-4) 0;
    padding: 0;
  }

  li,
  .loghead {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-2) 0;
    border-bottom: 1px solid var(--k-border);
  }

  .grow {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .test {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    margin: var(--k-space-4) 0;
  }

  .loghead {
    justify-content: space-between;
    margin-top: var(--k-space-4);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--k-font-size-sm);
  }

  td {
    padding: var(--k-space-1) var(--k-space-3);
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
