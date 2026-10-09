// KeyManager (SPEC §4, ARCHITECTURE §9.4): chord → action routing with the tmux-style app prefix,
// terminal passthrough rules and the disabled webview shortcuts.
//
// Routing rules:
// - Kelta chords and the prefix key are consumed (preventDefault + stopPropagation; the xterm
//   custom key handler returns false). Everything else reaches the focused element untouched.
// - Matching is on `KeyboardEvent.code` (physical keys). After the prefix, the next key is matched
//   on `KeyboardEvent.key` (characters such as `%`, `"`, `(`, `N`).
// - Default chords never use plain Ctrl+letter, Alt/Meta chords, Ctrl+Alt, Shift+Tab, Ctrl+Space or
//   Ctrl+\ on Linux (checked by the unit tests against the generated catalog).

import type { KeysSettings } from '$lib/gen';
import { ACTIONS, type ActionContext } from '$lib/gen/actions';
import { currentPlatform } from '$lib/ui/format';

import {
  chordFromEvent,
  chordToString,
  matchChord,
  parseChord,
  type Chord,
  type Platform,
} from './chords';

export interface KeyContext {
  /** Focus is inside a terminal view. */
  terminal: boolean;
  /** Session kind name (`claude`, `editor`, `shell`, `tool`, …) of the focused terminal. */
  kind: string | null;
  /** Focus is in a text field outside a terminal. */
  textInput: boolean;
  /** A sheet, dialog, palette or menu is open. */
  overlay: boolean;
}

export type KeyResult = 'consumed' | 'pass';

export interface ExtraBinding {
  action: string;
  chords: readonly string[];
  args?: Record<string, unknown>;
}

export interface KeyManagerOptions {
  platform?: Platform;
  /** Effective `[keys]` settings (null before they are loaded: catalog defaults apply). */
  keys?: () => KeysSettings | null;
  /** Runs an action; the default is `dispatch` of `$lib/actions`. */
  dispatch?: (id: string, args?: Record<string, unknown>) => unknown;
  /** Whether an action has a handler (unhandled actions do not consume the key). */
  hasAction?: (id: string) => boolean;
  /** Focus context of an event; the default inspects `event.target`. */
  context?: (event: KeyboardEvent) => KeyContext;
  /** Extra chords (tool keybindings), re-read on `refresh()`. */
  extraBindings?: () => readonly ExtraBinding[];
  onError?: (err: unknown, actionId: string) => void;
  /** Timer functions (tests inject fakes). The prefix timeout is the only timer: event-armed. */
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

interface Binding {
  action: string;
  context: ActionContext;
  args?: Record<string, unknown>;
}

interface Compiled {
  chords: Map<string, Binding>;
  prefix: Chord | null;
  prefixText: string;
  timeoutMs: number;
  prefixKeys: Map<string, Binding>;
}

const DEFAULT_PREFIX = 'ctrl+shift+space';
const DEFAULT_TIMEOUT_MS = 1000;
const ARROW_PREFIX_KEYS: Record<string, string> = {
  'pane.focus_left': 'ArrowLeft',
  'pane.focus_down': 'ArrowDown',
  'pane.focus_up': 'ArrowUp',
  'pane.focus_right': 'ArrowRight',
};
/** Actions that stay available while an overlay (sheet, dialog, palette) is open. */
const OVERLAY_ACTIONS = new Set(['palette.open', 'project.switcher']);
const BARE_MODIFIER_KEYS = new Set(['Shift', 'Control', 'Alt', 'Meta', 'AltGraph', 'CapsLock', 'OS']);

/** Effective chords of an action: `keys.bindings` override, else the platform catalog default. */
export function effectiveChords(
  actionId: string,
  keys: Pick<KeysSettings, 'bindings'> | null,
  platform: Platform = currentPlatform(),
): readonly string[] {
  const override = keys?.bindings?.[actionId];
  if (override) return override;
  const meta = ACTIONS.find((a) => a.id === actionId);
  return meta ? (platform === 'macos' ? meta.mac : meta.linux) : [];
}

/** Every action → effective chords (catalog defaults ⊕ overrides, plus unknown ids from overrides). */
export function effectiveBindings(
  keys: Pick<KeysSettings, 'bindings'> | null,
  platform: Platform = currentPlatform(),
): Record<string, readonly string[]> {
  const out: Record<string, readonly string[]> = {};
  for (const a of ACTIONS) out[a.id] = effectiveChords(a.id, keys, platform);
  for (const [id, chords] of Object.entries(keys?.bindings ?? {})) if (!(id in out)) out[id] = chords;
  return out;
}

export function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const tag = target.tagName;
  return (
    tag === 'INPUT' ||
    tag === 'TEXTAREA' ||
    tag === 'SELECT' ||
    (target as HTMLElement).isContentEditable === true
  );
}

/** Default context: derived from the event target and the open overlays in the document. */
export function contextFromEvent(event: KeyboardEvent): KeyContext {
  const target = event.target instanceof Element ? event.target : null;
  const host = target?.closest<HTMLElement>('[data-terminal]') ?? null;
  return {
    terminal: host !== null,
    kind: host?.dataset.kind ?? null,
    textInput: host === null && isEditable(target),
    overlay: typeof document !== 'undefined' && document.querySelector('[aria-modal="true"]') !== null,
  };
}

function contextAllows(context: ActionContext, ctx: KeyContext): boolean {
  switch (context) {
    case 'global':
      return true;
    case 'terminal':
      return ctx.terminal;
    case 'editor':
      return ctx.terminal && ctx.kind === 'editor';
    case 'ticket_views':
      return !ctx.terminal && !ctx.textInput;
    case 'external':
      return false;
  }
}

export class KeyManager {
  #opts: KeyManagerOptions;
  #platform: Platform;
  #compiled: Compiled | null = null;
  #armed = false;
  #timer: unknown = null;
  #listeners = new Set<(armed: boolean) => void>();

  constructor(options: KeyManagerOptions = {}) {
    this.#opts = options;
    this.#platform = options.platform ?? currentPlatform();
  }

  get platform(): Platform {
    return this.#platform;
  }

  /** The app prefix was pressed and the next key selects an action. */
  get prefixArmed(): boolean {
    return this.#armed;
  }

  /** Listens to prefix state changes (status bar indicator). Returns an unsubscribe fn. */
  onPrefixChange(listener: (armed: boolean) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /** Drops the compiled bindings (call when `[keys]`, tools or plugins change). */
  refresh(): void {
    this.#compiled = null;
    this.#disarm();
  }

  /** Starts listening on `target` (window by default). Returns a stop function. */
  start(target: EventTarget = window): () => void {
    const onKeyDown = (e: Event): void => {
      const event = e as KeyboardEvent;
      // Terminal views route their own events through attachCustomKeyEventHandler.
      if (event.target instanceof Element && event.target.closest('[data-terminal]')) return;
      this.handleKeyDown(event);
    };
    const onWheel = (e: Event): void => {
      // Webview zoom (Ctrl/Cmd + wheel) is disabled.
      const w = e as WheelEvent;
      if (w.ctrlKey || w.metaKey) w.preventDefault();
    };
    const onContextMenu = (e: Event): void => {
      // The default webview context menu is disabled except in text fields and selectable text.
      const t = e.target instanceof Element ? e.target : null;
      if (t && (isEditable(t) || t.closest('.k-selectable'))) return;
      e.preventDefault();
    };
    target.addEventListener('keydown', onKeyDown, { capture: true });
    target.addEventListener('wheel', onWheel, { capture: true, passive: false });
    target.addEventListener('contextmenu', onContextMenu);
    return () => {
      target.removeEventListener('keydown', onKeyDown, { capture: true });
      target.removeEventListener('wheel', onWheel, { capture: true });
      target.removeEventListener('contextmenu', onContextMenu);
      this.#disarm();
    };
  }

  /**
   * Routes one keydown. Returns `consumed` when Kelta handled it (the event is already
   * `preventDefault`-ed and stopped); the xterm custom key handler maps this to `return false`.
   */
  handleKeyDown(event: KeyboardEvent): KeyResult {
    if (event.type !== 'keydown' || event.isComposing || event.keyCode === 229) return 'pass';
    const compiled = this.#compile();

    if (this.#armed) return this.#handlePrefixKey(event, compiled);

    if (compiled.prefix && matchChord(compiled.prefix, event)) {
      this.#arm(compiled.timeoutMs);
      return this.#consume(event);
    }

    const chord = chordFromEvent(event);
    if (!chord) return 'pass';
    const binding = compiled.chords.get(chordToString(chord));
    if (binding) {
      const ctx = this.#context(event);
      if (this.#runnable(binding, ctx)) {
        this.#run(binding);
        return this.#consume(event);
      }
    }
    return this.#blockWebviewDefault(event, chord);
  }

  // ---- internals ----------------------------------------------------------------------------

  #context(event: KeyboardEvent): KeyContext {
    return (this.#opts.context ?? contextFromEvent)(event);
  }

  #runnable(binding: Binding, ctx: KeyContext): boolean {
    if (ctx.overlay && !OVERLAY_ACTIONS.has(binding.action)) return false;
    if (!contextAllows(binding.context, ctx)) return false;
    const has = this.#opts.hasAction;
    return has ? has(binding.action) : true;
  }

  #run(binding: Binding): void {
    const dispatch = this.#opts.dispatch;
    if (!dispatch) return;
    try {
      const result = dispatch(binding.action, binding.args);
      if (result instanceof Promise) result.catch((err: unknown) => this.#opts.onError?.(err, binding.action));
    } catch (err) {
      this.#opts.onError?.(err, binding.action);
    }
  }

  #consume(event: KeyboardEvent): KeyResult {
    event.preventDefault();
    event.stopPropagation();
    return 'consumed';
  }

  #handlePrefixKey(event: KeyboardEvent, compiled: Compiled): KeyResult {
    if (BARE_MODIFIER_KEYS.has(event.key)) return 'pass'; // holding Shift/Ctrl while choosing is fine
    this.#disarm();
    if (event.key === 'Escape' || (compiled.prefix && matchChord(compiled.prefix, event))) {
      return this.#consume(event);
    }
    const binding = this.#prefixBinding(event, compiled);
    if (binding) {
      const ctx = this.#context(event);
      if (this.#runnable(binding, ctx)) this.#run(binding);
    }
    // Unknown keys are swallowed too: a half-typed prefix sequence must not leak into the terminal.
    return this.#consume(event);
  }

  #prefixBinding(event: KeyboardEvent, compiled: Compiled): Binding | undefined {
    const direct = compiled.prefixKeys.get(event.key);
    if (direct) return direct;
    if (event.shiftKey && /^[A-Z]$/.test(event.key)) {
      const lower = compiled.prefixKeys.get(event.key.toLowerCase());
      if (lower) return lower;
    }
    const digit = /^Digit([0-9])$/.exec(event.code);
    return digit ? compiled.prefixKeys.get(digit[1]!) : undefined;
  }

  /** Webview defaults (reload, zoom, find) are disabled; plain Linux Ctrl chords still reach the PTY. */
  #blockWebviewDefault(event: KeyboardEvent, chord: Chord): KeyResult {
    if (!isWebviewDefault(chord, this.#platform)) return 'pass';
    const ctx = this.#context(event);
    // Linux: Ctrl+R / Ctrl+F belong to the shell inside a terminal.
    if (ctx.terminal && this.#platform === 'linux') return 'pass';
    return this.#consume(event);
  }

  #arm(timeoutMs: number): void {
    this.#clear();
    this.#timer = (this.#opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms)))(() => this.#disarm(), timeoutMs); // one-shot: prefix timeout
    this.#setArmed(true);
  }

  #disarm(): void {
    this.#clear();
    this.#setArmed(false);
  }

  #clear(): void {
    if (this.#timer !== null) {
      (this.#opts.clearTimer ?? ((h) => clearTimeout(h as ReturnType<typeof setTimeout>)))(this.#timer);
      this.#timer = null;
    }
  }

  #setArmed(armed: boolean): void {
    if (this.#armed === armed) return;
    this.#armed = armed;
    for (const l of [...this.#listeners]) l(armed);
  }

  #compile(): Compiled {
    if (this.#compiled) return this.#compiled;
    const keys = this.#opts.keys?.() ?? null;
    const platform = this.#platform;
    const chords = new Map<string, Binding>();
    const add = (text: string, binding: Binding): void => {
      const chord = parseChord(text, platform);
      if (!chord) return;
      const key = chordToString(chord);
      if (!chords.has(key)) chords.set(key, binding);
    };
    for (const meta of ACTIONS) {
      if (meta.context === 'external') continue;
      for (const text of effectiveChords(meta.id, keys, platform)) {
        add(text, { action: meta.id, context: meta.context });
      }
    }
    for (const [id, list] of Object.entries(keys?.bindings ?? {})) {
      if (ACTIONS.some((a) => a.id === id)) continue;
      for (const text of list) add(text, { action: id, context: 'global' });
    }
    for (const extra of this.#opts.extraBindings?.() ?? []) {
      for (const text of extra.chords) add(text, { action: extra.action, context: 'global', args: extra.args });
    }

    const prefixText = keys?.prefix ?? DEFAULT_PREFIX;
    const prefix = prefixText === 'off' ? null : parseChord(prefixText, platform);
    const timeoutMs = Math.min(5000, Math.max(200, keys?.prefix_timeout_ms ?? DEFAULT_TIMEOUT_MS));

    const prefixKeys = new Map<string, Binding>();
    for (const meta of ACTIONS) {
      if (meta.context === 'external') continue;
      const key = keys?.prefix_bindings?.[meta.id] ?? meta.prefix;
      const binding: Binding = { action: meta.id, context: meta.context };
      if (key && !prefixKeys.has(key)) prefixKeys.set(key, binding);
      const arrow = ARROW_PREFIX_KEYS[meta.id];
      if (arrow) prefixKeys.set(arrow, binding);
    }
    for (const [id, key] of Object.entries(keys?.prefix_bindings ?? {})) {
      if (ACTIONS.some((a) => a.id === id)) continue;
      if (!prefixKeys.has(key)) prefixKeys.set(key, { action: id, context: 'global' });
    }

    this.#compiled = { chords, prefix, prefixText, timeoutMs, prefixKeys };
    return this.#compiled;
  }
}

/** Webview default shortcuts that Kelta disables: reload, zoom and find. */
export function isWebviewDefault(chord: Chord, platform: Platform): boolean {
  const mod = platform === 'macos' ? chord.meta : chord.ctrl;
  if (chord.code === 'F5') return !chord.alt;
  if (!mod || chord.alt) return false;
  if (platform === 'linux' && chord.meta) return false;
  return (
    chord.code === 'KeyR' ||
    chord.code === 'KeyF' ||
    chord.code === 'Equal' ||
    chord.code === 'Minus' ||
    chord.code === 'Digit0' ||
    chord.code === 'NumpadAdd' ||
    chord.code === 'NumpadSubtract'
  );
}
