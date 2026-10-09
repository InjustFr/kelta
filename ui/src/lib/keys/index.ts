// SCAFFOLD STUB (L2): key manager (SPEC §4, ARCHITECTURE §9.4). L4's keybinding editor imports
// `parseChord` and `findConflicts`; the stub versions are inert (no chords, no conflicts).

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
  /** Action ids bound to the chord, or `reserved` when the chord is reserved for terminals. */
  actions: string[];
  reserved: boolean;
}

/** Parses `ctrl+shift+k` / `cmd+opt+left` / `mod+t`. Returns null when invalid. */
export function parseChord(text: string, platform: 'macos' | 'linux' = 'linux'): Chord | null {
  void text;
  void platform;
  return null;
}

export function matchChord(chord: Chord, event: KeyboardEvent): boolean {
  void chord;
  void event;
  return false;
}

/** Conflicts between bindings (action id → chords) and the reserved chord list. */
export function findConflicts(
  bindings: Record<string, readonly string[]>,
  reserved: readonly string[],
): Conflict[] {
  void bindings;
  void reserved;
  return [];
}

export class KeyManager {
  /** Starts listening on `target` (window by default). Returns a stop function. */
  start(target: EventTarget = window): () => void {
    void target;
    return () => {};
  }
}
