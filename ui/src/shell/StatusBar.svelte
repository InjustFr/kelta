<script lang="ts">
  import { dispatch } from '$lib/actions';
  import type { SessionInfo } from '$lib/gen';
  import { lampOf, sessions, settings, type LampLevel } from '$lib/stores';
  import { Kbd, Lamp } from '$lib/ui';

  import { attentionLabel, chordFor } from './labels';
  import { focusedSession, focusedSessionId, revealSession } from './nav';

  interface Props {
    prefixArmed: boolean;
    inbox: boolean;
  }

  let { prefixArmed, inbox }: Props = $props();

  // The hint line teaches the keyboard: the user's own chords, skipped when unbound.
  const HINTS: { id: string; verb: string }[] = [
    { id: 'palette.open', verb: 'Commands' },
    { id: 'session.new', verb: 'New session' },
    { id: 'pane.split_right', verb: 'Split' },
    { id: 'pane.close', verb: 'Close pane' },
    { id: 'attention.next', verb: 'Next waiting' },
  ];
  const hints = $derived(
    HINTS.map((h) => ({ ...h, chord: chordFor(h.id) })).filter(
      (h): h is { id: string; verb: string; chord: string } => !!h.chord,
    ),
  );

  const session = $derived(inbox ? null : focusedSession());
  const hooksOff = $derived(
    session?.kind.type === 'claude' && session.lifecycle === 'live' && session.claude?.hooks_active !== true,
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

  function words(lamp: LampLevel, n: number): string {
    if (lamp === 'needs_input') return n === 1 ? `${n} needs input` : `${n} need input`;
    if (lamp === 'done') return `${n} ready to review`;
    return `${n} ${attentionLabel(lamp).toLowerCase()}`;
  }

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
</script>

<footer class="statusbar k-num" data-testid="statusbar">
  <p class="hints">
    {#each hints as h (h.id)}
      <span class="hint"><Kbd chord={h.chord} />{h.verb}</span>
    {/each}
  </p>
  {#if prefixArmed}
    <span class="seg prefix" data-testid="status-prefix" role="status">Prefix: press a key</span>
  {/if}
  {#if restart}
    <span class="seg warn" title="Some settings need a restart">Restart needed</span>
  {/if}
  {#if hooksOff}
    <span class="seg warn" data-testid="status-hooks">Live status off</span>
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
      {words(m.lamp, m.list.length)}
    </button>
  {/each}
</footer>

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

  .hints {
    display: flex;
    align-items: center;
    gap: var(--k-space-5);
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    flex: none;
  }

  .warn {
    color: var(--k-warn);
  }

  .prefix {
    color: var(--k-accent);
    font-weight: var(--k-weight-strong);
  }

  /* Full bar height so the target stays 28px. */
  .btn {
    align-self: stretch;
    padding: 0 var(--k-space-3);
    border: none;
    border-radius: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .btn:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }
</style>
