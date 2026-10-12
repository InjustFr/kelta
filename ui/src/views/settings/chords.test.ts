// Settings > Keys recorder: chords are stored as the letters the active layout types.
import { describe, expect, it } from 'vitest';

import { chordFromEvent } from './lib/chords';

describe('chordFromEvent', () => {
  it('records the layout letter, not the QWERTY position', () => {
    // AZERTY: the key labelled A sends code KeyQ.
    const e = new KeyboardEvent('keydown', { code: 'KeyQ', key: 'A', ctrlKey: true, shiftKey: true });
    expect(chordFromEvent(e)).toBe('ctrl+shift+a');
  });
});
