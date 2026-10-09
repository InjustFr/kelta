<script lang="ts">
  import { dispatch } from '$lib/actions';
  import { attention, projects, settings, work } from '$lib/stores';
  import { Icon } from '$lib/ui';

  import AttentionDot from './AttentionDot.svelte';
  import { statusLabel } from './labels';
  import { focusedSession } from './nav';
  import PerfHud from './PerfHud.svelte';

  interface Props {
    prefixArmed: boolean;
    inbox: boolean;
  }

  let { prefixArmed, inbox }: Props = $props();

  const project = $derived(inbox ? null : projects.active);
  const session = $derived(inbox ? null : focusedSession());
  const item = $derived(session?.work_item_id ? work.get(session.work_item_id) : null);
  const hooks = $derived(
    session?.kind.type === 'claude' && session.lifecycle === 'live'
      ? (session.claude?.hooks_active ?? false)
      : null,
  );
  const needsInput = $derived(attention.totalNeedsInput);
  const restart = $derived(settings.pendingRestart.length > 0);

  let hud = $state(false);
</script>

<footer class="statusbar" data-testid="statusbar">
  <span class="seg project" data-testid="status-project">
    {#if inbox}
      <Icon name="inbox" size={12} /> Inbox
    {:else if project}
      <span class="swatch" style:background={project.color ?? 'var(--k-border-strong)'}></span>
      {project.name}
    {/if}
  </span>
  {#if item}
    <span class="seg" data-testid="status-branch" title="Branch of the focused session"
      ><Icon name="git-branch" size={12} /> {item.branch}</span
    >
  {/if}
  {#if session}
    <span class="seg" data-testid="status-session" title={session.name}>
      <AttentionDot level={session.attention} size={7} />
      {session.name}{statusLabel(session.status) ? ` · ${statusLabel(session.status)}` : ''}
    </span>
  {/if}
  {#if hooks !== null}
    <span class="seg" class:warn={!hooks} data-testid="status-hooks"
      >{hooks ? 'hooks ok' : 'hooks inactive'}</span
    >
  {/if}
  <span class="grow"></span>
  {#if prefixArmed}
    <span class="seg prefix" data-testid="status-prefix" role="status">prefix…</span>
  {/if}
  {#if restart}
    <span class="seg warn" title="Some settings need a restart">restart needed</span>
  {/if}
  {#if needsInput > 0}
    <button
      type="button"
      class="seg btn needs"
      onclick={() => dispatch('attention.next')}
      title="Next session needing input"
      data-testid="status-needs-input"
    >
      <AttentionDot level="needs_input" size={7} />
      {needsInput} need{needsInput === 1 ? 's' : ''} input
    </button>
  {/if}
  <button
    type="button"
    class="seg btn"
    class:on={hud}
    onclick={() => (hud = !hud)}
    title="Performance (on demand)"
    aria-label="Performance"
    aria-expanded={hud}
    data-testid="status-perf"
  >
    <Icon name="cpu" size={12} />
  </button>
</footer>

{#if hud}
  <PerfHud onclose={() => (hud = false)} />
{/if}

<style>
  .statusbar {
    display: flex;
    align-items: center;
    gap: var(--k-space-4);
    height: var(--k-statusbar-height);
    flex: none;
    padding: 0 var(--k-space-4);
    border-top: 1px solid var(--k-border);
    background: var(--k-bg-elev);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    white-space: nowrap;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }

  .grow {
    flex: 1;
  }

  .warn {
    color: var(--k-warn);
  }

  .prefix {
    color: var(--k-accent);
    font-weight: 600;
  }

  .btn {
    padding: 0 var(--k-space-2);
    border: none;
    border-radius: var(--k-radius-sm);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  .btn:hover,
  .btn.on {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .needs {
    color: var(--k-att-needs-input);
  }
</style>
