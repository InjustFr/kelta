import { describe, expect, it } from 'vitest';

import { ACTIONS, RESERVED_CHORDS, RESERVED_CLASSES } from '$lib/gen/actions';

import {
  chordFromEvent,
  chordToString,
  findConflicts,
  isReservedChord,
  matchChord,
  parseChord,
  type Platform,
} from './chords';
import { effectiveBindings } from './manager';

const RESERVED = [...RESERVED_CHORDS, ...RESERVED_CLASSES];

function key(init: KeyboardEventInit & { code: string }): KeyboardEvent {
  return new KeyboardEvent('keydown', init);
}

describe('parseChord', () => {
  it('parses modifiers and physical keys', () => {
    expect(parseChord('ctrl+shift+k', 'linux')).toEqual({
      ctrl: true,
      alt: false,
      shift: true,
      meta: false,
      code: 'KeyK',
    });
    expect(parseChord('cmd+opt+left', 'macos')).toEqual({
      ctrl: false,
      alt: true,
      shift: false,
      meta: true,
      code: 'ArrowLeft',
    });
    expect(parseChord('Ctrl+Shift+1', 'linux')?.code).toBe('Digit1');
    expect(parseChord('ctrl+shift+]', 'linux')?.code).toBe('BracketRight');
    expect(parseChord('ctrl+shift+,', 'linux')?.code).toBe('Comma');
    expect(parseChord('ctrl+shift+pagedown', 'linux')?.code).toBe('PageDown');
    expect(parseChord('ctrl+shift+space', 'linux')?.code).toBe('Space');
    expect(parseChord('f5', 'linux')?.code).toBe('F5');
  });

  it('expands mod per platform', () => {
    expect(parseChord('mod+t', 'macos')).toMatchObject({ meta: true, ctrl: false, shift: false, code: 'KeyT' });
    expect(parseChord('mod+t', 'linux')).toMatchObject({ meta: false, ctrl: true, shift: true, code: 'KeyT' });
  });

  it('rejects invalid chords', () => {
    for (const bad of ['', 'ctrl', 'ctrl+', 'ctrl+shift', 'k+ctrl', 'ctrl+nope', 'ctrl++', 'a+b']) {
      expect(parseChord(bad, 'linux'), bad).toBeNull();
    }
  });

  it('round-trips through chordToString', () => {
    const chord = parseChord('shift+ctrl+K', 'linux')!;
    expect(chordToString(chord)).toBe('ctrl+shift+k');
    expect(chordToString(parseChord('cmd+shift+]', 'macos')!)).toBe('shift+meta+]');
  });
});

describe('matchChord', () => {
  it('matches exact modifiers on the physical key', () => {
    const chord = parseChord('ctrl+shift+1', 'linux')!;
    // Shifted digit on a US layout produces key "!" but the physical code stays Digit1.
    expect(matchChord(chord, key({ code: 'Digit1', key: '!', ctrlKey: true, shiftKey: true }))).toBe(true);
    // AZERTY: the digit row needs Shift, the code is still Digit1.
    expect(matchChord(chord, key({ code: 'Digit1', key: '&', ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(matchChord(chord, key({ code: 'Digit1', key: '1', ctrlKey: true }))).toBe(false);
    expect(
      matchChord(chord, key({ code: 'Digit1', key: '!', ctrlKey: true, shiftKey: true, altKey: true })),
    ).toBe(false);
    expect(matchChord(chord, key({ code: 'Digit2', key: '@', ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it('falls back to key when code is empty', () => {
    const chord = parseChord('cmd+k', 'macos')!;
    expect(matchChord(chord, key({ code: '', key: 'k', metaKey: true }))).toBe(true);
  });

  it('chordFromEvent ignores bare modifiers', () => {
    expect(chordFromEvent(key({ code: 'ShiftLeft', key: 'Shift', shiftKey: true }))).toBeNull();
    expect(chordFromEvent(key({ code: 'KeyA', key: 'a', ctrlKey: true }))?.code).toBe('KeyA');
  });
});

describe('reserved chords', () => {
  const linux: Platform = 'linux';
  const reserved = (text: string, platform: Platform = linux): boolean =>
    isReservedChord(parseChord(text, platform)!, RESERVED, platform);

  it('flags plain Ctrl+letter, Alt, Meta, Ctrl+Alt, Shift+Tab, Ctrl+Space, Ctrl+\\', () => {
    for (const c of [
      'ctrl+a',
      'ctrl+z',
      'alt+x',
      'alt+shift+f',
      'ctrl+alt+t',
      'meta+k',
      'shift+tab',
      'ctrl+space',
      'ctrl+\\',
    ]) {
      expect(reserved(c), c).toBe(true);
    }
  });

  it('does not flag Ctrl+Shift chords', () => {
    for (const c of ['ctrl+shift+k', 'ctrl+shift+1', 'ctrl+shift+pagedown', 'ctrl+shift+space', 'ctrl+enter']) {
      expect(reserved(c), c).toBe(false);
    }
  });

  it('treats Cmd chords as app chords on macOS', () => {
    expect(reserved('cmd+k', 'macos')).toBe(false);
    expect(reserved('cmd+alt+left', 'macos')).toBe(false);
    expect(reserved('alt+x', 'macos')).toBe(true);
    expect(reserved('ctrl+c', 'macos')).toBe(true);
  });
});

describe('findConflicts', () => {
  it('reports chords bound twice', () => {
    const out = findConflicts(
      { 'palette.open': ['ctrl+shift+k'], 'tab.next': ['ctrl+shift+K'], 'tab.prev': ['ctrl+shift+j'] },
      RESERVED,
      { platform: 'linux' },
    );
    expect(out).toEqual([{ chord: 'ctrl+shift+k', actions: ['palette.open', 'tab.next'], reserved: false }]);
  });

  it('reports reserved chords', () => {
    const out = findConflicts({ 'palette.open': ['ctrl+k'], 'pane.zoom': ['super+z'] }, RESERVED, {
      platform: 'linux',
    });
    expect(out).toEqual([
      { chord: 'ctrl+k', actions: ['palette.open'], reserved: true },
      { chord: 'meta+z', actions: ['pane.zoom'], reserved: true },
    ]);
  });

  it('ignores invalid chords and duplicates of the same action', () => {
    expect(
      findConflicts({ 'palette.open': ['ctrl+shift+k', 'ctrl+shift+k', 'nope'] }, RESERVED, { platform: 'linux' }),
    ).toEqual([]);
  });
});

describe('default chords of the generated catalog', () => {
  it('never take plain Ctrl+letter, Alt/Meta, Ctrl+Alt, Shift+Tab, Ctrl+Space or Ctrl+\\ on Linux', () => {
    const defaults = effectiveBindings(null, 'linux');
    expect(findConflicts(defaults, RESERVED, { platform: 'linux' })).toEqual([]);
    // Every Linux default parses and none carries Alt or Meta.
    for (const meta of ACTIONS) {
      for (const text of meta.linux) {
        const chord = parseChord(text, 'linux');
        expect(chord, `${meta.id}: ${text}`).not.toBeNull();
        expect(chord!.alt || chord!.meta, `${meta.id}: ${text}`).toBe(false);
      }
    }
  });

  it('only use app chords on macOS (Cmd based, never bare Ctrl/Alt)', () => {
    const defaults = effectiveBindings(null, 'macos');
    expect(findConflicts(defaults, RESERVED, { platform: 'macos' })).toEqual([]);
    for (const meta of ACTIONS) {
      for (const text of meta.mac) expect(parseChord(text, 'macos')!.meta, `${meta.id}: ${text}`).toBe(true);
    }
  });

  it('are unique per platform', () => {
    for (const platform of ['linux', 'macos'] as const) {
      const seen = new Map<string, string>();
      for (const [id, chords] of Object.entries(effectiveBindings(null, platform))) {
        for (const text of chords) {
          const canon = chordToString(parseChord(text, platform)!);
          expect(seen.get(canon), `${platform} ${canon}`).toBeUndefined();
          seen.set(canon, id);
        }
      }
    }
  });
});
