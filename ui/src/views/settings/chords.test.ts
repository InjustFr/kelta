// Settings > Keys recorder: chords are stored as the letters the active layout types.
import { describe, expect, it } from 'vitest';

import { ACTIONS } from '$lib/gen/actions';

import { actionArea, actionsOn, chordFromEvent, KEY_AREAS, matchesPress, matchesText } from './lib/chords';

describe('chordFromEvent', () => {
  it('records the layout letter, not the QWERTY position', () => {
    // AZERTY: the key labelled A sends code KeyQ.
    const e = new KeyboardEvent('keydown', { code: 'KeyQ', key: 'A', ctrlKey: true, shiftKey: true });
    expect(chordFromEvent(e)).toBe('ctrl+shift+a');
  });
});

describe('actionArea', () => {
  it('groups actions by id prefix, unknown ones under App', () => {
    const area = {
      'pane.split_right': 'panes',
      'session.new': 'panes',
      'project.goto.1': 'projects',
      'tickets.open': 'tickets',
      'reviews.open': 'tickets',
      'terminal.copy': 'terminal',
      'work.ship': 'work',
      'palette.open': 'app',
      'step.next': 'app',
      'plugin.command.x': 'app',
    };
    for (const [id, a] of Object.entries(area)) expect(actionArea(id), id).toBe(a);
    for (const a of ACTIONS) expect(KEY_AREAS.map((x) => x.id)).toContain(actionArea(a.id));
  });
});

describe('matchesText', () => {
  it('matches the label or the id, any case', () => {
    expect(matchesText('split', 'Split right', 'pane.split_right')).toBe(true);
    expect(matchesText('PANE.SPLIT', 'Split right', 'pane.split_right')).toBe(true);
    expect(matchesText('  ', 'Split right', 'pane.split_right')).toBe(true);
    expect(matchesText('zoom', 'Split right', 'pane.split_right')).toBe(false);
  });
});

describe('matchesPress', () => {
  it('matches a bound chord in any spelling', () => {
    expect(matchesPress({ chord: 'cmd+shift+d', key: null }, ['shift+cmd+d'], null)).toBe(true);
    expect(matchesPress({ chord: 'cmd+shift+d', key: null }, ['cmd+d'], null)).toBe(false);
  });

  it('matches the key after the prefix only without Cmd/Ctrl/Alt', () => {
    expect(matchesPress({ chord: "shift+'", key: '"' }, [], '"')).toBe(true);
    expect(matchesPress({ chord: 'cmd+d', key: null }, [], 'd')).toBe(false);
    expect(matchesPress({ chord: 'shift+n', key: 'N' }, [], 'n')).toBe(false);
  });
});

describe('actionsOn', () => {
  it('lists the other actions on a chord', () => {
    expect(actionsOn({ a: ['ctrl+d'], b: ['Ctrl+D', 'x'], c: ['y'] }, 'ctrl+d', 'a')).toEqual(['b']);
  });
});
