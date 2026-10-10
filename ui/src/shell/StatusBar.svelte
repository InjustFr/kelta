<script lang="ts">
  import { dispatch } from '$lib/actions';
  import type { SessionInfo } from '$lib/gen';
  import { lampOf, projects, sessions, settings, work, type LampLevel } from '$lib/stores';
  import { Icon, Lamp } from '$lib/ui';

  import { statusLabel } from './labels';
  import { focusedSession, focusedSessionId, revealSession } from './nav';
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
  // Lamp meter: one button per non-zero state; clicking walks the sessions in that state.
  const METER: { lamp: LampLevel; label: string }[] = [
    { lamp: 'needs_input', label: 'need input' },
    { lamp: 'error', label: 'in error' },
    { lamp: 'working', label: 'working' },
    { lamp: 'done', label: 'done' },
  ];
  const meter = $derived(
    METER.map((m) => ({
      ...m,
      list: sessions.all.filter((s) => lampOf(s.attention, s.status === 'working') === m.lamp),
    })).filter((m) => m.list.length > 0),
  );

  function walk(lamp: LampLevel, list: SessionInfo[]): void {
    if (lamp === 'needs_input') {
      void dispatch('attention.next');
      return;
    }
    const at = list.findIndex((s) => s.id === focusedSessionId());
    const next = list[(at + 1) % list.length];
    if (next) void revealSession(next.id);
  }
  const restart = $derived(settings.pendingRestart.length > 0);

  let hud = $state(false);
</script>

<footer class="statusbar k-num" data-testid="statusbar">
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
      ><Icon name="git-branch" size={12} /> <span class="k-mono">{item.branch}</span></span
    >
  {/if}
  {#if session}
    <span class="seg" data-testid="status-session" title={session.name}>
      <Lamp level={lampOf(session.attention, session.status === 'working')} />
      {session.name}
      {#if statusLabel(session.status)}<span class="state">{statusLabel(session.status)}</span>{/if}
    </span>
  {/if}
  {#if hooks !== null}
    <span class="seg" class:warn={!hooks} data-testid="status-hooks"
      >{hooks ? 'Hooks ok' : 'Hooks inactive'}</span
    >
  {/if}
  <span class="grow"></span>
  {#if prefixArmed}
    <span class="seg prefix" data-testid="status-prefix" role="status">prefix…</span>
  {/if}
  {#if restart}
    <span class="seg warn" title="Some settings need a restart">Restart needed</span>
  {/if}
  {#each meter as m (m.lamp)}
    <button
      type="button"
      class="seg btn"
      onclick={() => walk(m.lamp, m.list)}
      title="{m.list.length} {m.label}: go to the next one"
      aria-label="{m.list.length} {m.label}"
      data-testid={m.lamp === 'needs_input' ? 'status-needs-input' : `status-${m.lamp}`}
    >
      <Lamp level={m.lamp} title={m.label} />
      {m.list.length}
    </button>
  {/each}
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
    padding: 0 var(--k-space-3);
    background: var(--k-bezel-raised);
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-xs);
    white-space: nowrap;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .swatch {
    display: inline-block;
    width: 3px;
    height: 12px;
  }

  .state {
    color: var(--k-fg-subtle);
  }

  .grow {
    flex: 1;
  }

  .warn {
    color: var(--k-warn);
  }

  .prefix {
    color: var(--k-accent);
    font-weight: var(--k-weight-strong);
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
</style>
