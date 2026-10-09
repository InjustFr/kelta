import { describe, expect, it } from 'vitest';

import type { ShiftEnter } from '$lib/gen';

import {
  baseCharacter,
  isShiftEnter,
  macOptionIsMeta,
  optionMetaSequence,
  resolveShiftEnter,
  shiftEnterFor,
  shiftEnterSequence,
} from './keymap';

const enter = (init: Partial<KeyboardEvent> = {}) => ({
  key: 'Enter',
  shiftKey: true,
  ctrlKey: false,
  altKey: false,
  metaKey: false,
  ...init,
});

const DEFAULTS: Record<string, ShiftEnter> = { claude: 'esc-cr', default: 'passthrough' };

describe('Shift+Enter mapping', () => {
  it('sends ESC CR in Claude sessions and passes through elsewhere (defaults)', () => {
    expect(shiftEnterFor(enter(), DEFAULTS, 'claude')).toBe('\x1b\r');
    for (const kind of ['shell', 'editor', 'tool', 'setup', 'custom']) {
      expect(shiftEnterFor(enter(), DEFAULTS, kind), kind).toBeNull();
    }
  });

  it('follows the per-kind setting and the default entry', () => {
    const map: Record<string, ShiftEnter> = { default: 'newline', shell: 'esc-cr', editor: 'passthrough' };
    expect(shiftEnterFor(enter(), map, 'shell')).toBe('\x1b\r');
    expect(shiftEnterFor(enter(), map, 'tool')).toBe('\n');
    expect(shiftEnterFor(enter(), map, 'editor')).toBeNull();
    expect(resolveShiftEnter({}, 'claude')).toBe('passthrough');
    expect(resolveShiftEnter(undefined, 'claude')).toBe('passthrough');
  });

  it('only maps Shift+Enter without other modifiers', () => {
    expect(isShiftEnter(enter())).toBe(true);
    expect(isShiftEnter(enter({ shiftKey: false }))).toBe(false);
    expect(isShiftEnter(enter({ ctrlKey: true }))).toBe(false);
    expect(isShiftEnter(enter({ altKey: true }))).toBe(false);
    expect(isShiftEnter(enter({ metaKey: true }))).toBe(false);
    expect(isShiftEnter(enter({ key: 'a' }))).toBe(false);
  });

  it('sequences', () => {
    expect(shiftEnterSequence('esc-cr')).toBe('\x1b\r');
    expect(shiftEnterSequence('newline')).toBe('\n');
    expect(shiftEnterSequence('passthrough')).toBeNull();
  });
});

describe('Option as Meta', () => {
  const key = (code: string, init: Partial<KeyboardEvent> = {}) => ({
    code,
    altKey: true,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...init,
  });

  it('xterm handles `both` natively, `none` leaves composition alone', () => {
    expect(macOptionIsMeta('both')).toBe(true);
    expect(macOptionIsMeta('none')).toBe(false);
    expect(macOptionIsMeta('left')).toBe(false);
    expect(optionMetaSequence(key('KeyB'), 'both', true, true)).toBeNull();
    expect(optionMetaSequence(key('KeyB'), 'none', true, true)).toBeNull();
  });

  it('left-only: ESC-prefixes keys while the left Option is held', () => {
    expect(optionMetaSequence(key('KeyB'), 'left', true, false)).toBe('\x1bb');
    expect(optionMetaSequence(key('KeyB', { shiftKey: true }), 'left', true, false)).toBe('\x1bB');
    expect(optionMetaSequence(key('Digit3'), 'left', true, false)).toBe('\x1b3');
    expect(optionMetaSequence(key('Period'), 'left', true, false)).toBe('\x1b.');
    // The right Option composes characters as usual.
    expect(optionMetaSequence(key('KeyB'), 'left', false, true)).toBeNull();
  });

  it('right-only mirrors left-only', () => {
    expect(optionMetaSequence(key('KeyF'), 'right', false, true)).toBe('\x1bf');
    expect(optionMetaSequence(key('KeyF'), 'right', true, false)).toBeNull();
  });

  it('ignores Ctrl/Cmd chords, unknown keys and keys without Option', () => {
    expect(optionMetaSequence(key('KeyB', { ctrlKey: true }), 'left', true, false)).toBeNull();
    expect(optionMetaSequence(key('KeyB', { metaKey: true }), 'left', true, false)).toBeNull();
    expect(optionMetaSequence(key('ArrowLeft'), 'left', true, false)).toBeNull();
    expect(optionMetaSequence(key('KeyB', { altKey: false }), 'left', true, false)).toBeNull();
  });

  it('baseCharacter maps physical keys', () => {
    expect(baseCharacter('KeyZ', false)).toBe('z');
    expect(baseCharacter('KeyZ', true)).toBe('Z');
    expect(baseCharacter('Digit0', true)).toBeNull();
    expect(baseCharacter('Space', false)).toBe(' ');
    expect(baseCharacter('F1', false)).toBeNull();
  });
});
