// Terminal key remaps (SPEC §4 "Terminal key handling"): per-kind Shift+Enter and macOS
// Option-as-Meta for the left/right-only modes. Pure functions of the keyboard event so they are
// unit-testable without xterm.

import type { OptionAsMeta, ShiftEnter } from '$lib/gen';

/** Setting value for a session kind name (`claude`, `shell`, …), falling back to `default`. */
export function resolveShiftEnter(map: Readonly<Record<string, ShiftEnter>> | undefined, kind: string): ShiftEnter {
  return map?.[kind] ?? map?.default ?? 'passthrough';
}

/** Bytes sent for Shift+Enter, or null when the key passes through to xterm. */
export function shiftEnterSequence(mode: ShiftEnter): string | null {
  switch (mode) {
    case 'esc-cr':
      return '\x1b\r';
    case 'newline':
      return '\n';
    case 'passthrough':
      return null;
  }
}

/** `Shift+Enter` and nothing else (Ctrl/Alt/Meta+Enter are other keys). */
export function isShiftEnter(e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'ctrlKey' | 'altKey' | 'metaKey'>): boolean {
  return e.key === 'Enter' && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey;
}

/** Sequence for a Shift+Enter keydown in a session of `kind`, or null (passthrough / other key). */
export function shiftEnterFor(
  e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'ctrlKey' | 'altKey' | 'metaKey'>,
  map: Readonly<Record<string, ShiftEnter>> | undefined,
  kind: string,
): string | null {
  return isShiftEnter(e) ? shiftEnterSequence(resolveShiftEnter(map, kind)) : null;
}

const PUNCTUATION: Record<string, string> = {
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
  Space: ' ',
};

/** Base character of a physical key (US layout), used when Option composition must be bypassed. */
export function baseCharacter(code: string, shift: boolean): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return shift ? letter[1]! : letter[1]!.toLowerCase();
  if (shift) return null;
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1]!;
  return PUNCTUATION[code] ?? null;
}

/** `macOptionIsMeta` for xterm: only `both` can be expressed natively. */
export function macOptionIsMeta(mode: OptionAsMeta): boolean {
  return mode === 'both';
}

/**
 * Left/right-only Option-as-Meta (macOS): when the chosen Option key is held, the key is sent as
 * `ESC <char>` instead of the composed character. Returns null when xterm should handle the key.
 */
export function optionMetaSequence(
  e: Pick<KeyboardEvent, 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>,
  mode: OptionAsMeta,
  leftOptionDown: boolean,
  rightOptionDown: boolean,
): string | null {
  if (mode !== 'left' && mode !== 'right') return null;
  if (!e.altKey || e.ctrlKey || e.metaKey) return null;
  const metaHeld = mode === 'left' ? leftOptionDown : rightOptionDown;
  if (!metaHeld) return null;
  const ch = baseCharacter(e.code, e.shiftKey);
  return ch === null ? null : `\x1b${ch}`;
}
