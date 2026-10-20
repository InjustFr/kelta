// KeyManager (SPEC §4, ARCHITECTURE §9.4): chord → action routing with the tmux-style app prefix,
// terminal passthrough rules and the disabled webview shortcuts.
//
// Routing rules:
// - Kelta chords and the prefix key are consumed (preventDefault + stopPropagation; the xterm
//   custom key handler returns false). Everything else reaches the focused element untouched.
// - Letters and punctuation match the character of the active layout, digits and named keys the
//   physical key (`eventCode`). After the prefix, the next key is matched on `KeyboardEvent.key`
//   (characters such as `%`, `"`, `(`, `N`).
// - `Esc` `Esc` in a tool's terminal hides the tool (`tools.hide`); the first `Esc` reaches it. Outside
//   terminals, fields and overlays it leaves focus mode (`view.unfocus`, registered only while it is on).
// - Linux, keys.linux_modifier = "ctrl_shift" (default): every default chord holds Ctrl+Shift (catalog `linux`), so a plain
//   Ctrl key reaches a focused terminal; a user/tool plain Ctrl+letter gets a Ctrl+Shift twin and yields to the terminal unless
//   keys.linux_terminal = "kelta_first". Ctrl+Shift+C/X/V edit text fields.
// - "super" (macKeymap): the Mac set with Super for Cmd, no twins; Super+A/C/X/V/Z, Super+Shift+Z edit text fields.

import type { KeysSettings } from '$lib/gen';
import { ACTIONS, type ActionContext } from '$lib/gen/actions';
import { msg } from '$lib/i18n/index.svelte';
import { currentPlatform } from '$lib/ui/format';
import { macKeymap } from '$lib/ui/keymap.svelte';

import {
  chordFromEvent,
  chordToString,
  isReservedChord,
  matchChord,
  parseChord,
  type Chord,
  type Platform,
} from './chords';

/** Two `Esc` presses this close together are one `Esc` `Esc`. */
const ESCAPE_TWICE_MS = 400;

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
  /** Reads the clipboard for a text-field paste on Linux (Ctrl+Shift+V, or Super+V with `linux_modifier = "super"`). */
  readClipboard?: () => Promise<string>;
  onError?: (err: unknown, actionId: string) => void;
  /** Timer functions (tests inject fakes). The prefix timer (hint or timeout) is the only one: event-armed. */
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
  hintMs: number;
  prefixKeys: Map<string, Binding>;
}

/** Groups of the which-key panel, in display order. */
export type HintGroup = 'goto' | 'work' | 'panes' | 'tools';

export interface PrefixHint {
  key: string;
  action: string;
  /** Untranslated, from `HINT_TABLE`. */
  label: string;
  /** `key` is a chord (a tool's), not a key after the prefix. */
  chord?: true;
}

const DEFAULT_PREFIX = 'ctrl+shift+space';
const DEFAULT_TIMEOUT_MS = 1000;
const DEFAULT_HINT_MS = 400;
/** `?` in the which-key panel, when no prefix binding takes it: Settings › Keys lists every key. */
const KEY_LIST: Binding = { action: 'settings.open', context: 'global', args: { section: 'keys' } };
const HINT_GROUPS: readonly HintGroup[] = ['goto', 'work', 'panes', 'tools'];

/**
 * The which-key panel (#436): per group at most 6 lines in plain words. A line is the actions that
 * share it (several: one `first–last` key range) and its label; the key comes from the effective prefix
 * binding, so a rebind keeps its place. Every other prefix binding stays bound but is not listed (`?`).
 * `tools` has the configured tools (WhichKey) before these.
 */
export const HINT_TABLE: Record<HintGroup, readonly { actions: readonly string[]; label: string }[]> = {
  goto: [
    { actions: ['inbox.open'], label: msg('Queue') },
    { actions: ['attention.next'], label: msg('Next that needs you') },
    { actions: ['tickets.open'], label: msg('Tickets') },
    { actions: ['reviews.open'], label: msg('Pull requests') },
    { actions: ['palette.open'], label: msg('Search') },
    { actions: Array.from({ length: 9 }, (_, i) => `project.goto.${i + 1}`), label: msg('Work row') },
  ],
  work: [
    { actions: ['work.next'], label: msg('Next step') },
    { actions: ['work.go_claude'], label: msg('Claude pane') },
    { actions: ['work.review_delta'], label: msg('Changes since you looked') },
    { actions: ['work.ship'], label: msg('Ship / Push') },
    { actions: ['work.edit_note'], label: msg('Note for later') },
  ],
  panes: [
    { actions: ['pane.split_right'], label: msg('Split right') },
    { actions: ['pane.split_down'], label: msg('Split down') },
    { actions: ['pane.zoom'], label: msg('Zoom pane') },
    { actions: ['pane.close'], label: msg('Close pane') },
    { actions: ['view.focus'], label: msg('Focus mode') },
  ],
  tools: [{ actions: ['settings.open'], label: msg('Settings') }],
};

const ARROW_PREFIX_KEYS: Record<string, string> = {
  'pane.focus_left': 'ArrowLeft',
  'pane.focus_down': 'ArrowDown',
  'pane.focus_up': 'ArrowUp',
  'pane.focus_right': 'ArrowRight',
};
/** Actions that stay available while an overlay (sheet, dialog, palette) is open. */
const OVERLAY_ACTIONS = new Set(['palette.open', 'project.switcher']);
/** Text-field edit chords WebKitGTK does not bind (#541), per `keys.linux_modifier`. */
const FIELD_COMMANDS: Record<'ctrl_shift' | 'super', Record<string, string>> = {
  ctrl_shift: { 'ctrl+shift+c': 'copy', 'ctrl+shift+x': 'cut', 'ctrl+shift+v': 'paste' },
  super: {
    'meta+a': 'selectAll',
    'meta+c': 'copy',
    'meta+x': 'cut',
    'meta+v': 'paste',
    'meta+z': 'undo',
    'shift+meta+z': 'redo',
  },
};
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
  return meta ? (macKeymap(platform) ? meta.mac : meta.linux) : [];
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

/** A plain printable key outside text fields, terminals, menus and dialogs: a list/board shortcut. */
export function isListKey(event: KeyboardEvent, ctx: KeyContext): boolean {
  if (event.key.length !== 1 || event.key === ' ' || event.ctrlKey || event.metaKey || event.altKey)
    return false;
  if (ctx.terminal || ctx.textInput || ctx.overlay) return false;
  const target = event.target instanceof Element ? event.target : null;
  return !target?.closest('[role="menu"], [role="dialog"], [role="listbox"]');
}

function contextAllows(context: ActionContext, ctx: KeyContext): boolean {
  switch (context) {
    case 'global':
      return true;
    case 'terminal':
      return ctx.terminal;
    case 'editor':
      return ctx.terminal && ctx.kind === 'editor';
    case 'no_text_field':
      return !ctx.textInput;
    case 'external':
      return false;
  }
}

export class KeyManager {
  #opts: KeyManagerOptions;
  #platform: Platform;
  #compiled: Compiled | null = null;
  #armed = false;
  #hint = false;
  #armedCtx: KeyContext | null = null;
  #timer: unknown = null;
  #lastEscape = -Infinity;
  #listeners = new Set<(armed: boolean, hint: boolean) => void>();

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

  /** The prefix waited `keys.prefix_hint_ms`: the which-key panel shows, armed until a key or Esc. */
  get prefixHint(): boolean {
    return this.#hint;
  }

  /** Listens to prefix state changes (status bar indicator, which-key panel). Returns an unsubscribe fn. */
  onPrefixChange(listener: (armed: boolean, hint: boolean) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  /**
   * The prefix chord and the keys after it that can run in the focus of the prefix press, by group
   * (empty groups left out). `onWork`: the focused tab is a work item's, else no `work` group.
   */
  prefixHints(onWork: boolean): { prefix: string; groups: { group: HintGroup; hints: PrefixHint[] }[] } {
    const compiled = this.#compile();
    const ctx = this.#armedCtx;
    const keyOf = (action: string): string | undefined => {
      for (const [key, b] of compiled.prefixKeys) {
        if (b.action !== action || key.startsWith('Arrow')) continue; // Arrows: twins of h/j/k/l
        if (!ctx || this.#runnable(b, ctx)) return key === 'Enter' ? '↵' : key;
      }
      return undefined;
    };
    const groups = HINT_GROUPS.filter((g) => g !== 'work' || onWork).map((group) => ({
      group,
      hints: HINT_TABLE[group].flatMap(({ actions, label }) => {
        const keys = actions.flatMap((a) => keyOf(a) ?? []);
        if (keys.length === 0) return [];
        const key = keys.length > 1 ? `${keys[0]}–${keys.at(-1)}` : keys[0]!;
        return [{ key, action: actions[0]!, label }];
      }),
    }));
    return { prefix: compiled.prefixText, groups: groups.filter((g) => g.hints.length > 0) };
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
      // Terminal views route their own events through attachCustomKeyEventHandler; key recorders
      // ([data-key-capture], Settings › Keys) read the raw press.
      if (event.target instanceof Element && event.target.closest('[data-terminal], [data-key-capture]'))
        return;
      if (this.handleKeyDown(event) === 'pass' && this.#opts.keys?.()?.list_keys === false) {
        // `keys.list_keys = false`: the letter shortcuts of lists and boards never reach their pane.
        if (isListKey(event, this.#context(event))) event.stopPropagation();
      }
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
      this.#armedCtx = this.#context(event);
      this.#arm(compiled);
      return this.#consume(event);
    }

    if (this.#escapeTwice(event)) return this.#consume(event);

    const chord = chordFromEvent(event);
    if (!chord) return 'pass';
    const binding = compiled.chords.get(chordToString(chord));
    if (binding) {
      const ctx = this.#context(event);
      if (this.#runnable(binding, ctx) && !this.#terminalKeeps(chord, ctx)) {
        // `reviews.open` only refreshes the pane in focus when the chord itself (not the leader key or
        // the palette) ran it (#417).
        this.#run(
          binding.action === 'reviews.open'
            ? { ...binding, args: { ...binding.args, chord: true } }
            : binding,
        );
        return this.#consume(event);
      }
    }
    if (this.#fieldEdit(event, chord)) return this.#consume(event);
    return this.#blockWebviewDefault(event, chord);
  }

  // ---- internals ----------------------------------------------------------------------------

  #context(event: KeyboardEvent): KeyContext {
    return (this.#opts.context ?? contextFromEvent)(event);
  }

  /**
   * `Esc` `Esc` in a tool's terminal runs `tools.hide` (#412), outside terminals and fields
   * `view.unfocus` (#421); the first `Esc` still reaches the program or the page.
   */
  #escapeTwice(event: KeyboardEvent): boolean {
    const plain =
      event.key === 'Escape' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey;
    const ctx = plain ? this.#context(event) : null;
    const binding: Binding | null =
      ctx?.kind === 'tool'
        ? { action: 'tools.hide', context: 'terminal' }
        : ctx && !ctx.terminal && !ctx.textInput && !ctx.overlay
          ? { action: 'view.unfocus', context: 'global' }
          : null;
    const armed = binding !== null && this.#runnable(binding, ctx!);
    const now = Date.now();
    const twice = armed && now - this.#lastEscape <= ESCAPE_TWICE_MS;
    this.#lastEscape = armed && !twice ? now : -Infinity;
    if (!twice) return false;
    this.#run(binding!);
    return true;
  }

  #runnable(binding: Binding, ctx: KeyContext): boolean {
    if (ctx.overlay && !OVERLAY_ACTIONS.has(binding.action)) return false;
    if (!contextAllows(binding.context, ctx)) return false;
    const has = this.#opts.hasAction;
    return has ? has(binding.action) : true;
  }

  /** Linux: a plain Ctrl+letter in a focused terminal is the shell's (`keys.linux_terminal`). */
  #terminalKeeps(chord: Chord, ctx: KeyContext): boolean {
    return (
      ctx.terminal &&
      this.#platform === 'linux' &&
      this.#opts.keys?.()?.linux_terminal !== 'kelta_first' &&
      isReservedChord(chord, ['ctrl+<letter>'], 'linux')
    );
  }

  /** Linux text fields: the edit chords of `FIELD_COMMANDS`; paste reads the clipboard and inserts with `insertText` (undoable). */
  #fieldEdit(event: KeyboardEvent, chord: Chord): boolean {
    if (this.#platform !== 'linux' || !this.#context(event).textInput) return false;
    const command = FIELD_COMMANDS[macKeymap('linux') ? 'super' : 'ctrl_shift'][chordToString(chord)];
    if (!command) return false;
    if (command !== 'paste') {
      document.execCommand(command);
      return true;
    }
    const read = this.#opts.readClipboard;
    if (!read) return false;
    const field = event.target;
    read().then(
      (text) => {
        if (document.activeElement === field) document.execCommand('insertText', false, text);
      },
      (err: unknown) => this.#opts.onError?.(err, 'terminal.paste'),
    );
    return true;
  }

  #run(binding: Binding): void {
    const dispatch = this.#opts.dispatch;
    if (!dispatch) return;
    try {
      const result = dispatch(binding.action, binding.args);
      if (result instanceof Promise)
        result.catch((err: unknown) => this.#opts.onError?.(err, binding.action));
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
    const hint = this.#hint;
    this.#disarm();
    if (event.key === 'Escape' || (compiled.prefix && matchChord(compiled.prefix, event))) {
      return this.#consume(event);
    }
    const binding =
      this.#prefixBinding(event, compiled) ?? (hint && event.key === '?' ? KEY_LIST : undefined);
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

  #arm({ timeoutMs, hintMs }: Compiled): void {
    this.#clear();
    const set = this.#opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    // One-shot, only while armed: the which-key panel (then armed until a key or Esc), else the timeout.
    this.#timer =
      hintMs > 0 && hintMs < timeoutMs
        ? set(() => {
            this.#timer = null;
            this.#setState(true, true);
          }, hintMs)
        : set(() => this.#disarm(), timeoutMs);
    this.#setState(true, false);
  }

  #disarm(): void {
    this.#clear();
    this.#setState(false, false);
  }

  #clear(): void {
    if (this.#timer !== null) {
      (this.#opts.clearTimer ?? ((h) => clearTimeout(h as ReturnType<typeof setTimeout>)))(this.#timer);
      this.#timer = null;
    }
  }

  #setState(armed: boolean, hint: boolean): void {
    if (this.#armed === armed && this.#hint === hint) return;
    this.#armed = armed;
    this.#hint = hint;
    for (const l of [...this.#listeners]) l(armed, hint);
  }

  #compile(): Compiled {
    if (this.#compiled) return this.#compiled;
    const keys = this.#opts.keys?.() ?? null;
    const platform = this.#platform;
    const chords = new Map<string, Binding>();
    // Linux: only the catalog ships a Ctrl+Shift twin; a user or tool Ctrl+letter gets one here,
    // so it still fires in a terminal that keeps plain Ctrl+letter (registered last, never shadows).
    const twins: [Chord, Binding][] = [];
    const add = (text: string, binding: Binding, twin = true): void => {
      const chord = parseChord(text, platform);
      if (!chord) return;
      const key = chordToString(chord);
      if (!chords.has(key)) chords.set(key, binding);
      if (twin && !macKeymap(platform) && isReservedChord(chord, ['ctrl+<letter>'], 'linux'))
        twins.push([{ ...chord, shift: true }, binding]);
    };
    for (const meta of ACTIONS) {
      if (meta.context === 'external') continue;
      const overridden = !!keys?.bindings?.[meta.id];
      for (const text of effectiveChords(meta.id, keys, platform)) {
        add(text, { action: meta.id, context: meta.context }, overridden);
      }
    }
    for (const [id, list] of Object.entries(keys?.bindings ?? {})) {
      if (ACTIONS.some((a) => a.id === id)) continue;
      for (const text of list) add(text, { action: id, context: 'global' });
    }
    for (const extra of this.#opts.extraBindings?.() ?? []) {
      for (const text of extra.chords)
        add(text, { action: extra.action, context: 'global', args: extra.args });
    }
    for (const [chord, binding] of twins) {
      const key = chordToString(chord);
      if (!chords.has(key)) chords.set(key, binding);
    }

    const prefixText = keys?.prefix ?? DEFAULT_PREFIX;
    const prefix = prefixText === 'off' ? null : parseChord(prefixText, platform);
    const timeoutMs = Math.min(5000, Math.max(200, keys?.prefix_timeout_ms ?? DEFAULT_TIMEOUT_MS));
    const hintMs = Math.min(5000, Math.max(0, keys?.prefix_hint_ms ?? DEFAULT_HINT_MS));

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

    this.#compiled = { chords, prefix, prefixText, timeoutMs, hintMs, prefixKeys };
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
