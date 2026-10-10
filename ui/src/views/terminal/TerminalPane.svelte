<script lang="ts">
  // Terminal pane: shows the pooled xterm view of one session (ARCHITECTURE §9). The view is created
  // and attached by the pool; this component owns the container, the exit banner, the attach error
  // state, the search bar and the context menu.
  import type { PaneProps } from '$app/registry';
  import { confirms } from '../../shell/confirm.svelte';
  import { closePaneById, openContent } from '../../shell/nav';
  import { sessions, settings, toasts, ui } from '$lib/stores';
  import { terminalPool } from '$lib/terminal';
  import { terminalUi } from '$lib/terminal/ui.svelte';
  import type { TerminalView, ViewState } from '$lib/terminal/view';
  import { dispatch } from '$lib/actions';
  import { Button, currentPlatform, EmptyState, ErrorState, Kbd, Menu, type MenuItem } from '$lib/ui';

  import SearchBar from './SearchBar.svelte';

  let { projectId, tabId, paneId, content, visible, focused }: PaneProps<'terminal'> = $props();

  const sessionId = $derived(content.session_id);
  const session = $derived(sessions.get(sessionId));

  let container = $state<HTMLDivElement>();
  let view = $state<TerminalView | null>(null);
  let viewState = $state<ViewState>({
    attaching: true,
    attached: false,
    exited: false,
    exitCode: null,
    error: null,
  });
  let menu = $state<{ x: number; y: number } | null>(null);

  // Parked (#142) or not restored yet: the next attach respawns it (`claude --resume`, nvim -S).
  const dormant = $derived(session?.lifecycle === 'dormant');
  const exited = $derived(!dormant && (viewState.exited || session?.lifecycle === 'exited'));
  const exitCode = $derived(viewState.exitCode ?? session?.exit_code ?? null);
  // The session was killed or removed: nothing to show (and nothing to attach to).
  const missing = $derived(sessions.loaded && !session);
  const searching = $derived(terminalUi.searchSession === sessionId);

  // Show the pooled view while the pane is visible; hide (not destroy) it otherwise.
  $effect(() => {
    if (!container || !visible || missing) return;
    const id = sessionId;
    const el = container;
    let alive = true;
    let off: (() => void) | null = null;
    terminalPool
      .show(id, el)
      .then(() => {
        if (!alive) return;
        const v = terminalPool.get<TerminalView>(id);
        if (!v) return;
        view = v;
        viewState = v.state;
        off = v.onState((s) => (viewState = s));
        v.confirmPaste = askPaste;
        v.onRequest = (r) => (r === 'restart' ? void restart() : close());
        // A pooled view of a session parked since: showing it again resumes the session.
        if (sessions.get(id)?.lifecycle === 'dormant' && v.state.attached) void v.reattach();
        if (focused) v.focus();
        void runProbe();
      })
      .catch((err: unknown) => {
        if (alive) {
          viewState = {
            attaching: false,
            attached: false,
            exited: false,
            exitCode: null,
            error: { code: 'internal', message: err instanceof Error ? err.message : String(err) },
          };
        }
      });
    return () => {
      alive = false;
      off?.();
      const v = terminalPool.get<TerminalView>(id);
      if (v) {
        v.confirmPaste = null;
        v.onRequest = null;
      }
      view = null;
      terminalPool.hide(id);
    };
  });

  // Keyboard focus follows the focused pane.
  $effect(() => {
    if (focused && visible && view) view.focus();
  });

  // Viewing a session marks it as seen (clears "done" / "activity" attention).
  $effect(() => {
    const s = session;
    if (visible && focused && ui.focused && s && !s.seen) void sessions.markSeen(s.id).catch(() => {});
  });

  // Linux, first run, `terminal.renderer = auto`: suggest WebGL when it measurably renders better.
  async function runProbe(): Promise<void> {
    const { maybeRunRenderProbe } = await import('$lib/terminal/probe');
    await maybeRunRenderProbe({
      platform: currentPlatform(),
      renderer: settings.value()?.terminal.renderer ?? 'auto',
      onSuggest: (r) =>
        toasts.push(
          {
            level: 'info',
            text: `WebGL renders ${Math.round(r.webglFps)} fps against ${Math.round(r.domFps)} fps for the DOM renderer here.`,
            action: { label: 'Use WebGL', command: 'terminal.set_renderer', args: { renderer: 'webgl' } },
          },
          { timeoutMs: 0 },
        ),
    });
  }

  async function askPaste(text: string): Promise<boolean> {
    const lines = text.replace(/[\r\n]+$/, '').split(/\r\n|\r|\n/).length;
    const answer = await confirms.ask({
      title: 'Paste multiple lines?',
      body: `The text has ${lines} lines and the program is not using bracketed paste, so each newline will run as Enter.`,
      details: text.split(/\r\n|\r|\n/).slice(0, 5),
      actions: [{ id: 'paste', label: 'Paste', variant: 'primary' }],
    });
    return answer === 'paste';
  }

  function close(): void {
    closePaneById(projectId, tabId, paneId);
  }

  async function restart(): Promise<void> {
    try {
      await sessions.restart(sessionId);
      await view?.reattach();
    } catch (err) {
      toasts.error(err, 'Restarting the session failed');
    }
  }

  async function retryAttach(): Promise<void> {
    await view?.reattach();
  }

  function closeSearch(): void {
    terminalUi.searchSession = null;
    view?.clearSearch();
    view?.focus();
  }

  function menuItems(): MenuItem[] {
    return [
      { id: 'copy', label: 'Copy', icon: 'copy', disabled: !view?.term.hasSelection() },
      { id: 'paste', label: 'Paste', icon: 'clipboard' },
      { id: 'search', label: 'Search…', icon: 'search', separator: true },
      { id: 'clear', label: 'Clear screen', icon: 'trash-2' },
    ];
  }

  async function onMenu(id: string): Promise<void> {
    if (!view) return;
    if (id === 'copy') await view.copy();
    else if (id === 'paste') await view.paste();
    else if (id === 'search') terminalUi.searchSession = sessionId;
    else if (id === 'clear') view.term.clear();
    view.focus();
  }

  function oncontextmenu(e: MouseEvent): void {
    // Applications that track the mouse own the right click.
    if (!view || view.term.modes.mouseTrackingMode !== 'none') return;
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY };
  }
</script>

<div
  class="terminal"
  data-testid="terminal-pane"
  data-session-id={sessionId}
  data-attached={viewState.attached}
  {oncontextmenu}
  role="presentation"
>
  <div class="host" bind:this={container}></div>

  {#if searching && view}
    <SearchBar {view} onclose={closeSearch} />
  {/if}

  {#if missing}
    <div class="overlay" data-testid="terminal-missing">
      <EmptyState
        icon="square-terminal"
        title="This session is gone"
        body="Its process was stopped or removed."
      >
        {#snippet actions()}
          <Button icon="x" onclick={close}>Close pane</Button>
        {/snippet}
      </EmptyState>
    </div>
  {:else if viewState.error}
    <div class="overlay" data-testid="terminal-error">
      <ErrorState
        error={viewState.error.message}
        title={viewState.error.code === 'not_found' ? 'Program not found' : 'Could not start this session'}
        onretry={retryAttach}
      >
        {#snippet actions()}
          {#if viewState.error?.code === 'not_found'}
            <Button icon="settings" onclick={() => void dispatch('settings.open', { section: 'editors' })}
              >Open editor settings</Button
            >
          {/if}
          <Button
            icon="activity"
            onclick={() => openContent(projectId, { content: { kind: 'diagnostics' }, placement: 'new_tab' })}
            >Open diagnostics</Button
          >
        {/snippet}
      </ErrorState>
    </div>
  {:else if dormant}
    <div class="banner" role="status" data-testid="parked-banner">
      <span class="mark parked" aria-hidden="true"></span>
      {#if viewState.attaching}
        <span class="what">Resuming…</span>
      {:else}
        <span class="what">Parked.</span>
        <Button size="sm" variant="primary" icon="play" onclick={retryAttach}>Resume</Button>
      {/if}
    </div>
  {:else if exited}
    <div class="banner" role="status" data-testid="exit-banner">
      <span class="mark" aria-hidden="true"></span>
      <span class="what"
        >Exited{exitCode !== null ? (exitCode === -1 ? ' on a signal' : ` with code ${exitCode}`) : ''}.</span
      >
      <!-- The buttons carry the actions; the key hints drop out first when the pane is narrow. -->
      <span class="keys" aria-hidden="true">
        <span class="key"><Kbd chord="enter" /> Restart</span>
        <span class="key"><Kbd chord="x" /> Close</span>
      </span>
      <Button size="sm" variant="primary" icon="refresh-cw" onclick={restart}>Restart</Button>
      <Button size="sm" icon="x" onclick={close}>Close</Button>
    </div>
  {/if}
</div>

{#if menu}
  <Menu
    items={menuItems()}
    x={menu.x}
    y={menu.y}
    label="Terminal"
    onselect={(id) => void onMenu(id)}
    onclose={() => (menu = null)}
  />
{/if}

<style>
  .terminal {
    position: relative;
    width: 100%;
    height: 100%;
    background: var(--k-term-bg);
    overflow: hidden;
  }

  .host {
    position: absolute;
    inset: 0;
  }

  .banner {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-3);
    background: var(--k-bezel-raised);
    color: var(--k-fg);
    z-index: 3;
  }

  .mark {
    width: 7px;
    height: 7px;
    transform: rotate(45deg);
    background: var(--k-lamp-error);
  }

  .mark.parked {
    transform: none;
    border-radius: 50%;
    background: transparent;
    border: 1.5px solid var(--k-fg-muted);
  }

  .what {
    flex: none;
    white-space: nowrap;
  }

  /* One line tall: hints that do not fit wrap out of view, the buttons keep their place. */
  .keys {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0 var(--k-space-4);
    flex: 1;
    min-width: 0;
    height: var(--k-control-height-sm);
    overflow: hidden;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
    white-space: nowrap;
  }

  .key {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: var(--k-control-height-sm);
  }

  .overlay {
    position: absolute;
    inset: 0;
    z-index: 3;
    background: var(--k-well);
  }
</style>
