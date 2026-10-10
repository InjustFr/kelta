// Kitty keyboard protocol encoder (https://sw.kovidgoyal.net/kitty/keyboard-protocol/), ported from
// alacritty's `input/keyboard.rs` (the alacritty app, not alacritty_terminal). The Rust model owns the
// mode stack and sends the active flags in Keyboard frames; this only turns a KeyboardEvent into bytes.
// `null` = the legacy bytes xterm.js sends are already right (plain text, unmodified Tab/Enter/arrows…).

import { baseCharacter } from './keymap';

export const DISAMBIGUATE = 1;
export const REPORT_EVENT_TYPES = 2;
export const REPORT_ALTERNATE_KEYS = 4;
export const REPORT_ALL_KEYS = 8;
export const REPORT_TEXT = 16;

export type KittyKeyEvent = Pick<
  KeyboardEvent,
  'type' | 'key' | 'code' | 'location' | 'repeat' | 'shiftKey' | 'altKey' | 'ctrlKey' | 'metaKey'
>;

const SHIFT = 1;
const NUMPAD_LOCATION = 3;
const RIGHT_LOCATION = 2;

/** `KeyboardEvent.key` names (`ArrowUp`, `F5`…), as opposed to the text a key produces. */
const NAMED = /^[A-Z][A-Za-z0-9]+$/;

/** Named keys that produce text (C0 controls): legacy bytes unless disambiguated. */
const TEXT_KEYS = new Set(['Enter', 'Tab', 'Backspace', 'Escape']);

/** Keys with a legacy form: `CSI number ; mods ~` or `CSI 1 ; mods letter` (number omitted when bare). */
const LEGACY: Record<string, [string, string]> = {
  Insert: ['2', '~'],
  Delete: ['3', '~'],
  PageUp: ['5', '~'],
  PageDown: ['6', '~'],
  Home: ['', 'H'],
  End: ['', 'F'],
  ArrowUp: ['', 'A'],
  ArrowDown: ['', 'B'],
  ArrowRight: ['', 'C'],
  ArrowLeft: ['', 'D'],
  F1: ['', 'P'],
  F2: ['', 'Q'],
  F3: ['13', '~'],
  F4: ['', 'S'],
  F5: ['15', '~'],
  F6: ['17', '~'],
  F7: ['18', '~'],
  F8: ['19', '~'],
  F9: ['20', '~'],
  F10: ['21', '~'],
  F11: ['23', '~'],
  F12: ['24', '~'],
};

/** `CSI code u` keys (F13-F35 are computed). */
const FUNCTIONAL: Record<string, number> = {
  Tab: 9,
  Enter: 13,
  Escape: 27,
  Backspace: 127,
  ScrollLock: 57359,
  PrintScreen: 57361,
  Pause: 57362,
  ContextMenu: 57363,
  MediaPlay: 57428,
  MediaPause: 57429,
  MediaPlayPause: 57430,
  MediaStop: 57432,
  MediaFastForward: 57433,
  MediaRewind: 57434,
  MediaTrackNext: 57435,
  MediaTrackPrevious: 57436,
  MediaRecord: 57437,
  AudioVolumeDown: 57438,
  AudioVolumeUp: 57439,
  AudioVolumeMute: 57440,
};

/** Lock and modifier keys, reported only with REPORT_ALL_KEYS. Right-hand modifiers are +6. */
const MODIFIER_KEYS: Record<string, number> = {
  CapsLock: 57358,
  NumLock: 57360,
  Shift: 57441,
  Control: 57442,
  Alt: 57443,
  Meta: 57444, // the browser's Meta is the kitty "super" key (Cmd / Windows key)
  Super: 57444,
  Hyper: 57445,
};

const NUMPAD: Record<string, number> = {
  '0': 57399,
  '1': 57400,
  '2': 57401,
  '3': 57402,
  '4': 57403,
  '5': 57404,
  '6': 57405,
  '7': 57406,
  '8': 57407,
  '9': 57408,
  '.': 57409,
  '/': 57410,
  '*': 57411,
  '-': 57412,
  '+': 57413,
  Enter: 57414,
  '=': 57415,
  ',': 57416,
  ArrowLeft: 57417,
  ArrowRight: 57418,
  ArrowUp: 57419,
  ArrowDown: 57420,
  PageUp: 57421,
  PageDown: 57422,
  Home: 57423,
  End: 57424,
  Insert: 57425,
  Delete: 57426,
};

function isControl(text: string): boolean {
  const c = text.codePointAt(0)!;
  return text.length === 1 && (c < 0x20 || (c >= 0x7f && c <= 0x9f));
}

/**
 * Bytes for a keydown/keyup under the active kitty `flags`, or null when xterm's own (legacy)
 * handling is right. `altText = false` when Alt composes characters (macOS Option not used as Meta):
 * Alt then only modifies named keys.
 */
export function encodeKittyKey(e: KittyKeyEvent, flags: number, altText = true): string | null {
  // Alternate keys and associated text only refine sequences that the other flags produce.
  if (!(flags & (DISAMBIGUATE | REPORT_EVENT_TYPES | REPORT_ALL_KEYS))) return null;
  const release = e.type === 'keyup';
  const all = (flags & REPORT_ALL_KEYS) !== 0;
  const numpad = e.location === NUMPAD_LOCATION;
  const named = NAMED.test(e.key);
  const chars = [...e.key];
  const alt = e.altKey && (named || (altText && chars.length === 1));
  const mods = (e.shiftKey ? SHIFT : 0) | (alt ? 2 : 0) | (e.ctrlKey ? 4 : 0) | (e.metaKey ? 8 : 0);
  const eventType = flags & REPORT_EVENT_TYPES && (e.repeat || release) ? (release ? 3 : 2) : 0;

  if (release) {
    if (!(flags & REPORT_EVENT_TYPES)) return null;
    // Enter/Tab/Backspace releases stay silent so `reset` can still be typed after a crash.
    if (!all && TEXT_KEYS.has(e.key) && e.key !== 'Escape') return null;
  } else if (!all) {
    const disambiguate =
      (flags & DISAMBIGUATE) !== 0 &&
      (e.key === 'Escape' ||
        numpad ||
        (mods !== 0 && (mods !== SHIFT || ['Tab', 'Enter', 'Backspace'].includes(e.key))));
    // Text and unmodified legacy keys: xterm sends the same bytes (and honours DECCKM).
    if (!disambiguate && (!named || TEXT_KEYS.has(e.key) || (e.key in LEGACY && !eventType))) return null;
  }

  const text =
    flags & REPORT_TEXT && !release && !named && !e.ctrlKey && e.key !== '' && !isControl(e.key) ? e.key : '';
  let base: string | null = null;
  let final = 'u';
  const fn = /^F(\d+)$/.exec(e.key);
  if (numpad && e.key in NUMPAD) {
    base = String(NUMPAD[e.key]);
  } else if (named) {
    if (e.key in FUNCTIONAL) base = String(FUNCTIONAL[e.key]);
    else if (e.key in LEGACY) {
      const [n, f] = LEGACY[e.key]!;
      base = n || (mods || eventType ? '1' : '');
      final = f;
    } else if (fn && Number(fn[1]) >= 13 && Number(fn[1]) <= 35) base = String(57363 + Number(fn[1]));
    else if (all && e.key in MODIFIER_KEYS) {
      const right = e.location === RIGHT_LOCATION && !e.key.endsWith('Lock');
      base = String(MODIFIER_KEYS[e.key]! + (right ? 6 : 0));
    }
  } else if (chars.length === 1) {
    const shifted = e.key.codePointAt(0)!;
    let code = (e.shiftKey ? e.key.toLowerCase() : e.key).codePointAt(0)!;
    // Shift changed the key but not its case (`!` for `1`): report the unshifted key.
    if (e.shiftKey && code === shifted) code = baseCharacter(e.code, false)?.codePointAt(0) ?? code;
    base = flags & REPORT_ALTERNATE_KEYS && code !== shifted ? `${code}:${shifted}` : String(code);
  } else if (all && text) {
    base = '0'; // text without a single key (multi-code-point input)
  }
  if (base === null) return null;

  let seq = `\x1b[${base}`;
  if (eventType || mods || text) seq += `;${mods + 1}`;
  if (eventType) seq += `:${eventType}`;
  if (text) seq += `;${[...text].map((c) => c.codePointAt(0)).join(':')}`;
  return seq + final;
}
