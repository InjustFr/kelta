<script lang="ts">
  // Pane `claude_runs` (#272, #392): a session's Claude `Workflow` runs, live from its hooks, split beside
  // its terminal. Phases with their agents, the selected agent's transcript tail, totals and result once
  // the run ends, and the work items the session claimed (ticket, branch, PR).
  import { locale, msg, t, tn } from '$lib/i18n/index.svelte';
  import type { PaneProps } from '$app/registry';
  import type { ClaudeRun, TranscriptLine } from '$lib/gen';
  import { claudeRunTail } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { sessions, work } from '$lib/stores';
  import { EmptyState } from '$lib/ui';

  import { runStatusLabel } from '../../shell/labels';
  import { workKey } from '../work/phase';

  let { content }: PaneProps<'claude_runs'> = $props();
  const sessionId = $derived(content.session_id);

  const session = $derived(sessions.byId[sessionId]);
  const runs = $derived([...(sessions.runs[sessionId] ?? [])].reverse());
  const claimed = $derived(
    work.all.filter((w) => w.run?.session_id === sessionId || w.id === session?.work_item_id),
  );

  // A restored pane can show before anything else asked for the session's runs.
  $effect(() => void sessions.loadRuns(sessionId));

  let selected = $state<{ run: string; agent: string } | null>(null);
  let tail = $state<TranscriptLine[] | string | null>(null);

  // The tail is read on select and again on each update of its run (no polling).
  const selectedRun = $derived(selected ? runs.find((r) => r.run_id === selected!.run) : undefined);
  $effect(() => {
    const sel = selected;
    if (!sel || !selectedRun) return;
    let stale = false;
    claudeRunTail({ id: sessionId, run_id: sel.run, agent_id: sel.agent }).then(
      (lines) => !stale && (tail = lines),
      (err: unknown) => !stale && (tail = toIpcError('claude_run_tail', err).message),
    );
    return () => (stale = true);
  });

  function pick(run: string, agent: string): void {
    selected = selected?.run === run && selected.agent === agent ? null : { run, agent };
    tail = null;
  }

  function duration(ms: number): string {
    const s = Math.round(ms / 1000);
    return s >= 60 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${s}s`;
  }

  function totals(r: ClaudeRun): string {
    const parts = [];
    if (r.duration_ms != null) parts.push(duration(r.duration_ms));
    if (r.total_tokens != null) parts.push(t('{n} tokens', { n: r.total_tokens.toLocaleString(locale()) }));
    if (r.total_tool_calls != null) parts.push(tn('{n} tool call', '{n} tool calls', r.total_tool_calls));
    return parts.join(' · ');
  }
</script>

<div class="body" data-testid="workflow-runs">
  <h2>{t('Workflows')} · {session?.name ?? t('session')}</h2>
  {#if claimed.length}
    <ul class="claimed">
      {#each claimed as w (w.id)}
        <li>
          <code>{workKey(w)}</code> ⎇ {w.branch}
          <span class="muted">{w.worktree}</span>
          {#if w.pr_url}<a href={w.pr_url} target="_blank" rel="noreferrer">PR</a>{/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#each runs as run (run.run_id)}
    <section class="run" data-testid="workflow-run" data-status={run.status}>
      <h3>
        {run.name ?? run.run_id}
        <span class="status {run.status}">{runStatusLabel(run.status)}</span>
        {#if run.status !== 'running'}<span class="muted">{totals(run)}</span>{/if}
      </h3>
      {#each run.phases as phase (phase.title)}
        <div class="phase">
          <span class="title">{phase.title}</span>
          <span class="agents">
            {#each phase.agents as a (a.id)}
              {@const state = a.done
                ? msg('done')
                : run.status === 'running'
                  ? msg('running')
                  : msg('stopped')}
              <button
                class="agent {state}"
                class:selected={selected?.run === run.run_id && selected.agent === a.id}
                title={t(state)}
                data-testid="workflow-agent"
                data-state={state}
                onclick={() => pick(run.run_id, a.id)}
                ><span aria-hidden="true">{a.done ? '✓' : state === 'running' ? '●' : '■'}</span
                >{a.label}</button
              >
            {/each}
          </span>
        </div>
      {/each}
      {#if selected?.run === run.run_id}
        <div class="tail" data-testid="workflow-tail" aria-live="polite">
          {#if tail === null}
            <span class="muted">{t('Reading the transcript…')}</span>
          {:else if typeof tail === 'string'}
            <span class="error">{tail}</span>
          {:else if tail.length === 0}
            <span class="muted">{t('Nothing yet')}</span>
          {:else}
            {#each tail as line, i (i)}
              <div class="line {line.kind}">{line.text}</div>
            {/each}
          {/if}
        </div>
      {/if}
      {#each run.logs as log, i (i)}
        <div class="log">{log}</div>
      {/each}
      {#if run.result}
        <details>
          <summary>{t('Result')}</summary>
          <pre>{run.result}</pre>
        </details>
      {/if}
    </section>
  {:else}
    <EmptyState title={t('No workflow run in this session')} />
  {/each}
</div>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
    height: 100%;
    padding: var(--k-space-4);
    overflow: auto;
    box-sizing: border-box;
  }

  h2 {
    margin: 0;
    font-size: var(--k-font-size-lg);
  }

  .claimed {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--k-font-size-sm);
  }

  .muted {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    font-weight: normal;
  }

  h3 {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-2);
    font-size: var(--k-font-size);
  }

  .status {
    font-size: var(--k-font-size-sm);
    font-weight: normal;
    color: var(--k-fg-muted);
  }

  .status.running {
    color: var(--k-accent);
  }

  .status.killed {
    color: var(--k-danger);
  }

  .phase {
    display: flex;
    gap: var(--k-space-3);
    padding: var(--k-space-1) 0;
  }

  .phase .title {
    flex: none;
    width: 88px;
    color: var(--k-fg-muted);
  }

  .agents {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-1);
  }

  .agent {
    display: inline-flex;
    gap: var(--k-space-1);
    padding: 0 var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .agent.running span {
    color: var(--k-accent);
  }

  .agent.stopped span {
    color: var(--k-danger);
  }

  .agent.selected {
    border-color: var(--k-accent);
  }

  .tail {
    margin-top: var(--k-space-2);
    padding: var(--k-space-2);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .line.tool {
    color: var(--k-accent);
  }

  .line.result,
  .log {
    color: var(--k-fg-muted);
  }

  .line.error,
  .error {
    color: var(--k-danger);
  }

  pre {
    max-height: 240px;
    overflow: auto;
    font-size: var(--k-font-size-sm);
    white-space: pre-wrap;
  }
</style>
