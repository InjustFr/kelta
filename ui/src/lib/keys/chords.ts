// Chord parsing, physical-key matching and conflict detection (SPEC §4). Pure: no DOM state, no
// stores. Matching uses `KeyboardEvent.code` (physical keys) so keyboard layouts do not break digits
// or punctuation chords.

import { currentPlatform } from '$lib/ui/format';

export type Platform = 'macos' | 'linux';

export interface Chord {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  /** `KeyboardEvent.code` (physical key), e.g. `KeyK`, `Digit1`, `Enter`. */
  code: string;
}

export interface Conflict {
  chord: string;
  /** Action ids bound to the chord (more than one = clash between bindings). */
  actions: string[];
  /** The chord is reserved for terminals (plain Ctrl+letter, Alt/Meta chords, Shift+Tab, …). */
  reserved: boolean;
}

const MODIFIER_NAMES: Record<string, 'ctrl' | 'alt' | 'shift' | 'meta' | 'mod'> = {
  ctrl: 'ctrl',
  control: 'ctrl',
  alt: 'alt',
  opt: 'alt',
  option: 'alt',
  shift: 'shift',
  cmd: 'meta',
  command: 'meta',
  meta: 'meta',
  super: 'meta',
  win: 'meta',
  mod: 'mod',
};

/** Named (non letter/digit) keys → `KeyboardEvent.code`. */
const NAMED_CODES: Record<string, string> = {
  enter: 'Enter',
  return: 'Enter',
  space: 'Space',
  tab: 'Tab',
  escape: 'Escape',
  esc: 'Escape',
  backspace: 'Backspace',
  delete: 'Delete',
  del: 'Delete',
  insert: 'Insert',
  home: 'Home',
  end: 'End',
  pageup: 'PageUp',
  pagedown: 'PageDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  up: 'ArrowUp',
  down: 'ArrowDown',
  ',': 'Comma',
  comma: 'Comma',
  '.': 'Period',
  period: 'Period',
  '/': 'Slash',
  slash: 'Slash',
  '\\': 'Backslash',
  backslash: 'Backslash',
  ';': 'Semicolon',
  semicolon: 'Semicolon',
  "'": 'Quote',
  quote: 'Quote',
  '`': 'Backquote',
  backquote: 'Backquote',
  '-': 'Minus',
  minus: 'Minus',
  '=': 'Equal',
  equal: 'Equal',
  plus: 'Equal',
  '[': 'BracketLeft',
  ']': 'BracketRight',
};

const CODE_NAMES: Record<string, string> = {
  Enter: 'enter',
  Space: 'space',
  Tab: 'tab',
  Escape: 'escape',
  Backspace: 'backspace',
  Delete: 'delete',
  Insert: 'insert',
  Home: 'home',
  End: 'end',
  PageUp: 'pageup',
  PageDown: 'pagedown',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Backquote: '`',
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
};

/** Physical code of a key name (`k`, `1`, `f5`, `pagedown`, `]`), or null when unknown. */
export function keyNameToCode(name: string): string | null {
  const n = name.toLowerCase();
  if (/^[a-z]$/.test(n)) return `Key${n.toUpperCase()}`;
  if (/^[0-9]$/.test(n)) return `Digit${n}`;
  if (/^f([1-9]|1[0-9]|2[0-4])$/.test(n)) return n.toUpperCase();
  return NAMED_CODES[n] ?? null;
}

/** Canonical key name of a physical code (`KeyK` → `k`), the inverse of `keyNameToCode`. */
export function codeToKeyName(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code.toLowerCase();
  return CODE_NAMES[code] ?? code.toLowerCase();
}

/** Parses `ctrl+shift+k` / `cmd+opt+left` / `mod+t`. Returns null when invalid. */
export function parseChord(text: string, platform: Platform = currentPlatform()): Chord | null {
  const raw = text.trim().toLowerCase();
  if (raw === '') return null;
  // A chord ending in "+" (e.g. "ctrl++") cannot be expressed: use `plus`.
  const parts = raw.split('+').map((p) => p.trim());
  if (parts.some((p) => p === '')) return null;
  const chord: Chord = { ctrl: false, alt: false, shift: false, meta: false, code: '' };
  for (const [i, part] of parts.entries()) {
    const mod = MODIFIER_NAMES[part];
    const isLast = i === parts.length - 1;
    if (mod) {
      if (isLast) return null; // chord without a key
      if (mod === 'mod') {
        if (platform === 'macos') chord.meta = true;
        else {
          chord.ctrl = true;
          chord.shift = true;
        }
      } else chord[mod] = true;
      continue;
    }
    if (!isLast) return null; // key must come last
    const code = keyNameToCode(part);
    if (!code) return null;
    chord.code = code;
  }
  return chord.code ? chord : null;
}

/** Canonical text of a chord (`ctrl+alt+shift+meta+<key>`), stable for map lookups. */
export function chordToString(chord: Chord): string {
  const mods = [
    chord.ctrl ? 'ctrl' : '',
    chord.alt ? 'alt' : '',
    chord.shift ? 'shift' : '',
    chord.meta ? 'meta' : '',
  ].filter(Boolean);
  return [...mods, codeToKeyName(chord.code)].join('+');
}

/** Physical code of an event, falling back to `key` for virtual keyboards that omit `code`. */
function eventCode(event: Pick<KeyboardEvent, 'code' | 'key'>): string {
  if (event.code) return event.code;
  const key = event.key ?? '';
  return key.length === 1 ? (keyNameToCode(key) ?? '') : '';
}

export function matchChord(chord: Chord, event: KeyboardEvent): boolean {
  return (
    eventCode(event) === chord.code &&
    event.ctrlKey === chord.ctrl &&
    event.altKey === chord.alt &&
    event.shiftKey === chord.shift &&
    event.metaKey === chord.meta
  );
}

/** The chord an event represents (`null` for bare modifier presses). */
export function chordFromEvent(event: KeyboardEvent): Chord | null {
  const code = eventCode(event);
  if (code === '' || /^(Control|Alt|Shift|Meta|OS)(Left|Right)$/.test(code)) return null;
  return { ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey, meta: event.metaKey, code };
}

// ---- reserved chords (SPEC §4) --------------------------------------------------------------

/**
 * Whether a chord is reserved for terminals. `reserved` mixes exact chords (`shift+tab`,
 * `ctrl+space`, `ctrl+\`) and classes: `ctrl+<letter>` (plain Ctrl+letter), `alt+*`, `meta+*`,
 * `super+*`, `ctrl+alt+*`. On macOS Cmd (meta) chords never reach the PTY, so they are not reserved.
 */
export function isReservedChord(
  chord: Chord,
  reserved: readonly string[],
  platform: Platform = currentPlatform(),
): boolean {
  if (platform === 'macos' && chord.meta) return false;
  for (const entry of reserved) {
    const e = entry.trim().toLowerCase();
    switch (e) {
      case 'ctrl+<letter>':
        if (chord.ctrl && !chord.alt && !chord.shift && !chord.meta && /^Key[A-Z]$/.test(chord.code))
          return true;
        break;
      case 'alt+*':
        if (chord.alt) return true;
        break;
      case 'meta+*':
      case 'super+*':
        if (chord.meta) return true;
        break;
      case 'ctrl+alt+*':
        if (chord.ctrl && chord.alt) return true;
        break;
      default: {
        const parsed = parseChord(e, platform);
        if (parsed && chordToString(parsed) === chordToString(chord)) return true;
      }
    }
  }
  return false;
}

/**
 * Conflicts between bindings (action id → chords) and the reserved chord list: chords bound to
 * several actions, and chords that are reserved for terminals. Invalid chords are ignored.
 */
export function findConflicts(
  bindings: Record<string, readonly string[]>,
  reserved: readonly string[],
  opts: { platform?: Platform } = {},
): Conflict[] {
  const platform = opts.platform ?? currentPlatform();
  const byChord = new Map<string, { chord: Chord; actions: string[] }>();
  for (const [action, chords] of Object.entries(bindings)) {
    for (const text of chords) {
      const chord = parseChord(text, platform);
      if (!chord) continue;
      const key = chordToString(chord);
      const slot = byChord.get(key) ?? { chord, actions: [] };
      if (!slot.actions.includes(action)) slot.actions.push(action);
      byChord.set(key, slot);
    }
  }
  const out: Conflict[] = [];
  for (const [key, { chord, actions }] of byChord) {
    const isReserved = isReservedChord(chord, reserved, platform);
    if (actions.length > 1 || isReserved) out.push({ chord: key, actions, reserved: isReserved });
  }
  return out;
}
