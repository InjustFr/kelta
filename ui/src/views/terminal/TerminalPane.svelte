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
  import { Button, currentPlatform, ErrorState, Menu, type MenuItem } from '$lib/ui';

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

  const exited = $derived(viewState.exited || session?.lifecycle === 'exited');
  const exitCode = $derived(viewState.exitCode ?? session?.exit_code ?? null);
  const searching = $derived(terminalUi.searchSession === sessionId);

  // Show the pooled view while the pane is visible; hide (not destroy) it otherwise.
  $effect(() => {
    if (!container || !visible) return;
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
      { id: 'clear', label: 'Clear screen', icon: 'eraser' },
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

  {#if viewState.error}
    <div class="overlay" data-testid="terminal-error">
      <ErrorState
        error={viewState.error.message}
        title={viewState.error.code === 'not_found' ? 'Program not found' : 'Could not start this session'}
        onretry={retryAttach}
      >
        {#snippet actions()}
          <Button
            icon="activity"
            onclick={() => openContent(projectId, { content: { kind: 'diagnostics' }, placement: 'new_tab' })}
            >Open diagnostics</Button
          >
        {/snippet}
      </ErrorState>
    </div>
  {:else if exited}
    <div class="banner" role="status" data-testid="exit-banner">
      <span
        >Exited{exitCode !== null ? ` (${exitCode === -1 ? 'signal' : `code ${exitCode}`})` : ''} —
        <kbd>Enter</kbd>
        restart ·
        <kbd>x</kbd> close</span
      >
      <span class="spacer"></span>
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
    padding: var(--k-space-3) var(--k-space-4);
    border-top: 1px solid var(--k-border);
    background: var(--k-bg-elev);
    color: var(--k-fg-muted);
    z-index: 3;
  }

  .spacer {
    flex: 1;
  }

  kbd {
    padding: 0 4px;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    font-size: var(--k-font-size-xs);
  }

  .overlay {
    position: absolute;
    inset: 0;
    z-index: 3;
    background: var(--k-bg);
  }
</style>
