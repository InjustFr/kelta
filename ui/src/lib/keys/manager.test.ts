import { afterEach, describe, expect, it, vi } from 'vitest';

import type { KeysSettings } from '$lib/gen';

import { KeyManager, type KeyContext, type KeyManagerOptions } from './manager';

const GLOBAL: KeyContext = { terminal: false, kind: null, textInput: false, overlay: false };
const TERMINAL: KeyContext = { terminal: true, kind: 'shell', textInput: false, overlay: false };

function keys(over: Partial<KeysSettings> = {}): KeysSettings {
  return {
    prefix: 'ctrl+shift+space',
    prefix_timeout_ms: 1000,
    bindings: {},
    prefix_bindings: {},
    list_keys: true,
    ...over,
  };
}

function setup(over: Omit<Partial<KeyManagerOptions>, 'keys'> & { ctx?: KeyContext; keys?: KeysSettings } = {}) {
  const { ctx: initialCtx, keys: keysOverride, ...options } = over;
  const calls: { id: string; args?: Record<string, unknown> }[] = [];
  let ctx = initialCtx ?? GLOBAL;
  const manager = new KeyManager({
    platform: 'linux',
    keys: () => keysOverride ?? keys(),
    dispatch: (id, args) => void calls.push({ id, args }),
    hasAction: () => true,
    context: () => ctx,
    ...options,
  });
  return {
    manager,
    calls,
    setCtx: (c: KeyContext) => (ctx = c),
    ids: () => calls.map((c) => c.id),
  };
}

function press(init: KeyboardEventInit & { code: string; key: string }): KeyboardEvent {
  return new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
}

const ctrlShift = (code: string, key: string): KeyboardEvent =>
  press({ code, key, ctrlKey: true, shiftKey: true });

afterEach(() => vi.useRealTimers());

describe('direct chords (Linux)', () => {
  it('dispatches and consumes a default chord', () => {
    const { manager, ids } = setup();
    const e = ctrlShift('KeyK', 'K');
    expect(manager.handleKeyDown(e)).toBe('consumed');
    expect(e.defaultPrevented).toBe(true);
    expect(ids()).toEqual(['palette.open']);
  });

  it('matches digits on the physical key', () => {
    const { manager, ids } = setup();
    manager.handleKeyDown(ctrlShift('Digit2', '@'));
    manager.handleKeyDown(ctrlShift('Digit0', ')'));
    manager.handleKeyDown(ctrlShift('PageDown', 'PageDown'));
    expect(ids()).toEqual(['project.goto.2', 'inbox.open', 'project.next']);
  });

  it('honours overrides and unbinding', () => {
    const k = keys({ bindings: { 'palette.open': ['ctrl+shift+y'], 'project.switcher': [] } });
    const { manager, ids } = setup({ keys: k });
    expect(manager.handleKeyDown(ctrlShift('KeyK', 'K'))).toBe('pass');
    expect(manager.handleKeyDown(ctrlShift('KeyP', 'P'))).toBe('pass');
    expect(manager.handleKeyDown(ctrlShift('KeyY', 'Y'))).toBe('consumed');
    expect(ids()).toEqual(['palette.open']);
  });

  it('supports plugin command bindings from keys.bindings', () => {
    const { manager, calls } = setup({ keys: keys({ bindings: { 'plugin.command.acme/deploy': ['ctrl+shift+9'] } }) });
    // 9 is project.goto.9 by default: first binding wins, so use another key.
    const { manager: m2, calls: c2 } = setup({
      keys: keys({ bindings: { 'plugin.command.acme/deploy': ['ctrl+shift+f9'] } }),
    });
    m2.handleKeyDown(ctrlShift('F9', 'F9'));
    expect(c2.map((c) => c.id)).toEqual(['plugin.command.acme/deploy']);
    manager.handleKeyDown(ctrlShift('Digit9', '('));
    expect(calls[0]?.id).toBe('project.goto.9');
  });

  it('does not consume chords of actions without a handler', () => {
    const { manager, calls } = setup({ hasAction: (id) => id !== 'tickets.open' });
    expect(manager.handleKeyDown(ctrlShift('KeyJ', 'J'))).toBe('pass');
    expect(calls).toEqual([]);
  });

  it('passes extra (tool) bindings with their args', () => {
    const { manager, calls } = setup({
      extraBindings: () => [{ action: 'tools.open', chords: ['mod+g'], args: { tool_id: 'lazygit' } }],
    });
    manager.handleKeyDown(ctrlShift('KeyG', 'G'));
    expect(calls).toEqual([{ id: 'tools.open', args: { tool_id: 'lazygit' } }]);
  });
});

describe('terminal passthrough', () => {
  const letters = 'abcdefghijklmnopqrstuvwxyz'.split('');

  it('never consumes plain Ctrl+letter, Alt, Ctrl+Alt, Super, Shift+Tab, Ctrl+Space, Ctrl+\\', () => {
    for (const ctx of [GLOBAL, TERMINAL]) {
      const { manager, calls, setCtx } = setup({ ctx });
      setCtx(ctx);
      const events: KeyboardEvent[] = [];
      for (const l of letters) {
        const code = `Key${l.toUpperCase()}`;
        events.push(press({ code, key: l, ctrlKey: true }));
        events.push(press({ code, key: l, altKey: true }));
        events.push(press({ code, key: l, altKey: true, shiftKey: true }));
        events.push(press({ code, key: l, ctrlKey: true, altKey: true }));
        events.push(press({ code, key: l, metaKey: true }));
      }
      events.push(press({ code: 'Tab', key: 'Tab', shiftKey: true }));
      events.push(press({ code: 'Space', key: ' ', ctrlKey: true }));
      events.push(press({ code: 'Backslash', key: '\\', ctrlKey: true }));
      events.push(press({ code: 'KeyA', key: 'a' }));
      events.push(press({ code: 'Enter', key: 'Enter' }));
      events.push(press({ code: 'Enter', key: 'Enter', shiftKey: true }));
      for (const e of events) {
        // Ctrl+R / Ctrl+F reach the PTY inside terminals; outside they are webview defaults.
        const result = manager.handleKeyDown(e);
        const isWebviewDefault = !ctx.terminal && e.ctrlKey && !e.altKey && (e.code === 'KeyR' || e.code === 'KeyF');
        if (!isWebviewDefault) {
          expect(result, `${e.code} c${+e.ctrlKey}a${+e.altKey}s${+e.shiftKey}m${+e.metaKey}`).toBe('pass');
          expect(e.defaultPrevented).toBe(false);
        }
      }
      expect(calls).toEqual([]);
    }
  });

  it('keeps Ctrl+R and Ctrl+F for the shell inside a terminal', () => {
    const { manager } = setup({ ctx: TERMINAL });
    const r = press({ code: 'KeyR', key: 'r', ctrlKey: true });
    const f = press({ code: 'KeyF', key: 'f', ctrlKey: true });
    expect(manager.handleKeyDown(r)).toBe('pass');
    expect(manager.handleKeyDown(f)).toBe('pass');
    expect(r.defaultPrevented || f.defaultPrevented).toBe(false);
  });

  it('disables webview reload/zoom/find outside terminals', () => {
    const { manager } = setup();
    for (const init of [
      { code: 'F5', key: 'F5' },
      { code: 'KeyR', key: 'r', ctrlKey: true },
      { code: 'KeyF', key: 'f', ctrlKey: true },
      { code: 'Equal', key: '+', ctrlKey: true },
      { code: 'Minus', key: '-', ctrlKey: true },
      { code: 'Digit0', key: '0', ctrlKey: true },
    ]) {
      const e = press(init);
      expect(manager.handleKeyDown(e), init.code).toBe('consumed');
      expect(e.defaultPrevented).toBe(true);
    }
  });

  it('disables webview defaults on macOS even inside terminals', () => {
    const { manager } = setup({ platform: 'macos', ctx: TERMINAL });
    const e = press({ code: 'KeyR', key: 'r', metaKey: true });
    expect(manager.handleKeyDown(e)).toBe('consumed');
    // ...while Ctrl+R still reaches the PTY.
    expect(manager.handleKeyDown(press({ code: 'KeyR', key: 'r', ctrlKey: true }))).toBe('pass');
  });

  it('ignores IME composition', () => {
    const { manager, calls } = setup();
    const e = new KeyboardEvent('keydown', {
      code: 'KeyK',
      key: 'K',
      ctrlKey: true,
      shiftKey: true,
      isComposing: true,
    });
    expect(manager.handleKeyDown(e)).toBe('pass');
    expect(calls).toEqual([]);
  });
});

describe('context-dependent actions', () => {
  it('terminal.* only fire inside terminals, work.start only outside', () => {
    const { manager, ids, setCtx } = setup();
    const copy = (): KeyboardEvent => ctrlShift('KeyC', 'C');
    expect(manager.handleKeyDown(copy())).toBe('pass');
    setCtx(TERMINAL);
    expect(manager.handleKeyDown(copy())).toBe('consumed');
    const start = (): KeyboardEvent => press({ code: 'Enter', key: 'Enter', ctrlKey: true });
    expect(manager.handleKeyDown(start())).toBe('pass'); // terminal: Ctrl+Enter reaches the PTY
    setCtx(GLOBAL);
    expect(manager.handleKeyDown(start())).toBe('consumed');
    setCtx({ ...GLOBAL, textInput: true });
    expect(manager.handleKeyDown(start())).toBe('pass');
    expect(ids()).toEqual(['terminal.copy', 'work.start']);
  });

  it('editor.send_selection only fires in editor sessions', () => {
    const { manager, ids, setCtx } = setup({ ctx: TERMINAL });
    expect(manager.handleKeyDown(ctrlShift('KeyL', 'L'))).toBe('pass');
    setCtx({ terminal: true, kind: 'editor', textInput: false, overlay: false });
    expect(manager.handleKeyDown(ctrlShift('KeyL', 'L'))).toBe('consumed');
    expect(ids()).toEqual(['editor.send_selection']);
  });

  it('only palette and switcher toggles work while an overlay is open', () => {
    const { manager, ids } = setup({ ctx: { ...GLOBAL, overlay: true } });
    expect(manager.handleKeyDown(ctrlShift('KeyT', 'T'))).toBe('pass');
    expect(manager.handleKeyDown(ctrlShift('KeyK', 'K'))).toBe('consumed');
    expect(manager.handleKeyDown(ctrlShift('KeyP', 'P'))).toBe('consumed');
    expect(ids()).toEqual(['palette.open', 'project.switcher']);
  });

  it('macOS uses Cmd chords', () => {
    const { manager, ids } = setup({ platform: 'macos' });
    expect(manager.handleKeyDown(press({ code: 'KeyK', key: 'k', metaKey: true }))).toBe('consumed');
    expect(manager.handleKeyDown(press({ code: 'KeyK', key: 'k', ctrlKey: true, shiftKey: true }))).toBe('pass');
    expect(manager.handleKeyDown(press({ code: 'ArrowLeft', key: 'ArrowLeft', metaKey: true, altKey: true }))).toBe(
      'consumed',
    );
    expect(ids()).toEqual(['palette.open', 'pane.focus_left']);
  });
});

describe('prefix state machine', () => {
  const prefix = (): KeyboardEvent => ctrlShift('Space', ' ');
  const next = (key: string, init: Partial<KeyboardEventInit> = {}): KeyboardEvent =>
    press({ code: key.length === 1 ? `Key${key.toUpperCase()}` : key, key, ...init });

  it('arms on the prefix chord and runs the next key', () => {
    const { manager, ids } = setup();
    const states: boolean[] = [];
    manager.onPrefixChange((a) => states.push(a));
    expect(manager.handleKeyDown(prefix())).toBe('consumed');
    expect(manager.prefixArmed).toBe(true);
    expect(manager.handleKeyDown(next('p'))).toBe('consumed');
    expect(manager.prefixArmed).toBe(false);
    expect(ids()).toEqual(['project.switcher']);
    expect(states).toEqual([true, false]);
  });

  it('resolves characters, uppercase and arrows', () => {
    const { manager, ids } = setup();
    const seq: [string, KeyboardEventInit][] = [
      [':', { code: 'Semicolon' }],
      ['%', { code: 'Digit5', shiftKey: true }],
      ['"', { code: 'Quote', shiftKey: true }],
      ['(', { code: 'Digit9', shiftKey: true }],
      [')', { code: 'Digit0', shiftKey: true }],
      ['N', { code: 'KeyN', shiftKey: true }],
      ['n', { code: 'KeyN' }],
      ['ArrowLeft', { code: 'ArrowLeft' }],
      ['h', { code: 'KeyH' }],
      ['3', { code: 'Digit3' }],
      ['0', { code: 'Digit0' }],
    ];
    for (const [key, init] of seq) {
      manager.handleKeyDown(prefix());
      manager.handleKeyDown(press({ key, code: 'KeyX', ...init }));
    }
    expect(ids()).toEqual([
      'palette.open',
      'pane.split_right',
      'pane.split_down',
      'project.prev',
      'project.next',
      'tab.prev',
      'tab.next',
      'pane.focus_left',
      'pane.focus_left',
      'project.goto.3',
      'inbox.open',
    ]);
  });

  it('tolerates Shift being held from the prefix chord', () => {
    const { manager, ids } = setup();
    manager.handleKeyDown(prefix());
    manager.handleKeyDown(press({ code: 'KeyP', key: 'P', ctrlKey: true, shiftKey: true }));
    expect(ids()).toEqual(['project.switcher']);
  });

  it('times out after prefix_timeout_ms', () => {
    vi.useFakeTimers();
    const { manager, ids } = setup({
      setTimer: (fn, ms) => setTimeout(fn, ms),
      clearTimer: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
    });
    manager.handleKeyDown(prefix());
    vi.advanceTimersByTime(999);
    expect(manager.prefixArmed).toBe(true);
    vi.advanceTimersByTime(2);
    expect(manager.prefixArmed).toBe(false);
    // The key after the timeout is an ordinary key again.
    expect(manager.handleKeyDown(next('p'))).toBe('pass');
    expect(ids()).toEqual([]);
  });

  it('uses keys.prefix_timeout_ms and holds no timer once disarmed', () => {
    vi.useFakeTimers();
    const { manager } = setup({
      keys: keys({ prefix_timeout_ms: 300 }),
      setTimer: (fn, ms) => setTimeout(fn, ms),
      clearTimer: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
    });
    manager.handleKeyDown(prefix());
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(301);
    expect(manager.prefixArmed).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    manager.handleKeyDown(prefix());
    manager.handleKeyDown(next('p'));
    expect(vi.getTimerCount()).toBe(0);
  });

  it('cancels on Escape, on the prefix itself and swallows unknown keys', () => {
    const { manager, ids } = setup();
    manager.handleKeyDown(prefix());
    expect(manager.handleKeyDown(press({ code: 'Escape', key: 'Escape' }))).toBe('consumed');
    expect(manager.prefixArmed).toBe(false);

    manager.handleKeyDown(prefix());
    expect(manager.handleKeyDown(prefix())).toBe('consumed');
    expect(manager.prefixArmed).toBe(false);

    manager.handleKeyDown(prefix());
    expect(manager.handleKeyDown(next('q'))).toBe('consumed');
    expect(manager.prefixArmed).toBe(false);
    expect(ids()).toEqual([]);
  });

  it('keeps waiting through bare modifier presses', () => {
    const { manager, ids } = setup();
    manager.handleKeyDown(prefix());
    manager.handleKeyDown(press({ code: 'ShiftLeft', key: 'Shift', shiftKey: true }));
    expect(manager.prefixArmed).toBe(true);
    manager.handleKeyDown(next('t'));
    expect(ids()).toEqual(['tickets.open']);
  });

  it('is disabled with prefix = "off" and honours custom prefix and prefix_bindings', () => {
    const off = setup({ keys: keys({ prefix: 'off' }) });
    expect(off.manager.handleKeyDown(prefix())).toBe('pass');
    expect(off.manager.prefixArmed).toBe(false);

    const custom = setup({
      platform: 'linux',
      keys: keys({ prefix: 'ctrl+shift+b', prefix_bindings: { 'palette.open': ';' } }),
    });
    expect(custom.manager.handleKeyDown(prefix())).toBe('pass');
    custom.manager.handleKeyDown(ctrlShift('KeyB', 'B'));
    expect(custom.manager.prefixArmed).toBe(true);
    custom.manager.handleKeyDown(press({ code: 'Semicolon', key: ';' }));
    expect(custom.ids()).toEqual(['palette.open']);
  });

  it('respects action contexts after the prefix', () => {
    const { manager, ids, setCtx } = setup();
    manager.handleKeyDown(prefix());
    manager.handleKeyDown(next('['));
    expect(ids()).toEqual([]);
    setCtx(TERMINAL);
    manager.handleKeyDown(prefix());
    manager.handleKeyDown(press({ code: 'BracketLeft', key: '[' }));
    expect(ids()).toEqual(['terminal.copy']);
  });

  it('refresh() disarms and recompiles', () => {
    let current = keys();
    const calls: string[] = [];
    const m = new KeyManager({
      platform: 'linux',
      keys: () => current,
      dispatch: (id) => void calls.push(id),
      hasAction: () => true,
      context: () => GLOBAL,
    });
    m.handleKeyDown(prefix());
    expect(m.prefixArmed).toBe(true);
    current = keys({ bindings: { 'palette.open': ['ctrl+shift+y'] } });
    m.refresh();
    expect(m.prefixArmed).toBe(false);
    expect(m.handleKeyDown(ctrlShift('KeyY', 'Y'))).toBe('consumed');
    expect(calls).toEqual(['palette.open']);
  });
});
