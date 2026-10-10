import { describe, expect, it } from 'vitest';

import {
  DISAMBIGUATE,
  encodeKittyKey,
  REPORT_ALL_KEYS,
  REPORT_ALTERNATE_KEYS,
  REPORT_EVENT_TYPES,
  REPORT_TEXT,
  type KittyKeyEvent,
} from './kitty';

const CODES: Record<string, string> = { a: 'KeyA', A: 'KeyA', '!': 'Digit1', '1': 'Digit1', ' ': 'Space' };

function key(k: string, init: Partial<KittyKeyEvent> = {}): KittyKeyEvent {
  return {
    type: 'keydown',
    key: k,
    code: CODES[k] ?? k,
    location: 0,
    repeat: false,
    shiftKey: false,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    ...init,
  };
}

const D = DISAMBIGUATE;
const ALL = REPORT_ALL_KEYS;
const EV = REPORT_EVENT_TYPES;
const shift = { shiftKey: true };
const ctrl = { ctrlKey: true };
const up = { type: 'keyup' };

// [name, event, flags, expected bytes (null = xterm's legacy bytes are right)]
const CASES: [string, KittyKeyEvent, number, string | null][] = [
  ['flags 0: legacy', key('Escape'), 0, null],
  ['alternate keys alone change nothing', key('Escape'), REPORT_ALTERNATE_KEYS | REPORT_TEXT, null],
  ['Escape', key('Escape'), D, '\x1b[27u'],
  ['Shift+Enter', key('Enter', shift), D, '\x1b[13;2u'],
  ['Shift+Tab', key('Tab', shift), D, '\x1b[9;2u'],
  ['Ctrl+Backspace', key('Backspace', ctrl), D, '\x1b[127;5u'],
  ['plain Enter stays legacy', key('Enter'), D, null],
  ['plain text stays text', key('a'), D, null],
  ['Shift+a stays text', key('A', shift), D, null],
  ['Ctrl+a', key('a', ctrl), D, '\x1b[97;5u'],
  ['Ctrl+Shift+a', key('A', { ...shift, ...ctrl }), D, '\x1b[97;6u'],
  [
    'Ctrl+Shift+a, alternate key',
    key('A', { ...shift, ...ctrl }),
    D | REPORT_ALTERNATE_KEYS,
    '\x1b[97:65;6u',
  ],
  ['Alt+a', key('a', { altKey: true }), D, '\x1b[97;3u'],
  ['Ctrl+Space', key(' ', ctrl), D, '\x1b[32;5u'],
  ['Super+a', key('a', { metaKey: true }), D, '\x1b[97;9u'],
  ['F5 bare stays legacy', key('F5'), D, null],
  ['Ctrl+F5', key('F5', ctrl), D, '\x1b[15;5~'],
  ['F5, all keys', key('F5'), ALL, '\x1b[15~'],
  ['Ctrl+F1', key('F1', ctrl), D, '\x1b[1;5P'],
  ['Ctrl+F3 is CSI 13 ~', key('F3', ctrl), D, '\x1b[13;5~'],
  ['F13', key('F13'), D, '\x1b[57376u'],
  ['F35', key('F35'), D, '\x1b[57398u'],
  ['bare arrow stays legacy (DECCKM)', key('ArrowUp'), D, null],
  ['Shift+Up: xterm sends the same CSI 1;2A', key('ArrowUp', shift), D, null],
  ['Ctrl+Up', key('ArrowUp', ctrl), D, '\x1b[1;5A'],
  ['Ctrl+Alt+Left', key('ArrowLeft', { ...ctrl, altKey: true }), D, '\x1b[1;7D'],
  ['Ctrl+PageDown', key('PageDown', ctrl), D, '\x1b[6;5~'],
  ['arrow, all keys', key('ArrowDown'), ALL, '\x1b[B'],
  ['numpad digit', key('1', { location: 3 }), D, '\x1b[57400u'],
  ['numpad Enter', key('Enter', { location: 3 }), D, '\x1b[57414u'],
  ['text, all keys', key('a'), ALL, '\x1b[97u'],
  ['Shift+1 reports the unshifted key', key('!', shift), ALL, '\x1b[49;2u'],
  ['Shift+1, alternate key', key('!', shift), ALL | REPORT_ALTERNATE_KEYS, '\x1b[49:33;2u'],
  ['Enter, all keys', key('Enter'), ALL, '\x1b[13u'],
  ['left Shift press, all keys', key('Shift', { ...shift, location: 1 }), ALL, '\x1b[57441;2u'],
  ['right Ctrl press, all keys', key('Control', { ...ctrl, location: 2 }), ALL, '\x1b[57448;5u'],
  ['modifier keys need all keys', key('Shift', shift), D, null],
  ['CapsLock, all keys', key('CapsLock'), ALL, '\x1b[57358u'],
  ['keyup without event types', key('a', up), D, null],
  ['keyup a', key('a', up), D | EV, '\x1b[97;1:3u'],
  ['keyup Shift+Up', key('ArrowUp', { ...up, ...shift }), D | EV, '\x1b[1;2:3A'],
  ['keyup Escape', key('Escape', up), D | EV, '\x1b[27;1:3u'],
  ['keyup Enter is silent', key('Enter', up), D | EV, null],
  ['keyup Enter, all keys', key('Enter', up), ALL | EV, '\x1b[13;1:3u'],
  ['keyup left Shift, all keys', key('Shift', { ...up, location: 1 }), ALL | EV, '\x1b[57441;1:3u'],
  ['repeat arrow', key('ArrowUp', { repeat: true }), D | EV, '\x1b[1;1:2A'],
  ['repeat text stays text', key('a', { repeat: true }), D | EV, null],
  ['press has no event type', key('a', ctrl), D | EV, '\x1b[97;5u'],
  ['text, associated text', key('a'), ALL | REPORT_TEXT, '\x1b[97;1;97u'],
  ['Shift+a, associated text', key('A', shift), ALL | REPORT_TEXT, '\x1b[97;2;65u'],
  ['Ctrl+a has no text', key('a', ctrl), ALL | REPORT_TEXT, '\x1b[97;5u'],
  ['no text on release', key('a', up), ALL | EV | REPORT_TEXT, '\x1b[97;1:3u'],
  ['multi-code-point text', key('é'), ALL | REPORT_TEXT, '\x1b[0;1;101:769u'],
  ['dead keys are left to xterm', key('Dead', { altKey: true }), ALL, null],
];

describe('encodeKittyKey', () => {
  it.each(CASES)('%s', (_name, event, flags, expected) => {
    expect(encodeKittyKey(event, flags)).toBe(expected);
  });

  it('Alt is only a modifier of named keys when Option composes text (macOS)', () => {
    expect(encodeKittyKey(key('a', { altKey: true }), D, false)).toBeNull();
    expect(encodeKittyKey(key('ArrowLeft', { altKey: true }), D, false)).toBe('\x1b[1;3D');
  });
});
