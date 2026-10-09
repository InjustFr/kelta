// Chord strings (`ctrl+shift+k`, `cmd+alt+left`): capture from a KeyboardEvent (physical key, like
// the key manager) and helpers for the keybinding editor.

import { ACTIONS, type ActionMeta } from '$lib/gen/actions';

export type Platform = 'macos' | 'linux';

const CODE_NAMES: Record<string, string> = {
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
  PageUp: 'pageup',
  PageDown: 'pagedown',
  Home: 'home',
  End: 'end',
  Enter: 'enter',
  NumpadEnter: 'enter',
  Space: 'space',
  Tab: 'tab',
  Backspace: 'backspace',
  Delete: 'delete',
  Escape: 'escape',
  Comma: ',',
  Period: '.',
  Slash: '/',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Minus: '-',
  Equal: '=',
  Backquote: '`',
};

const MODIFIER_CODES = new Set([
  'ShiftLeft',
  'ShiftRight',
  'ControlLeft',
  'ControlRight',
  'AltLeft',
  'AltRight',
  'MetaLeft',
  'MetaRight',
  'OSLeft',
  'OSRight',
]);

/** Name of the physical key, or null for a bare modifier / unknown key. */
export function keyName(code: string): string | null {
  if (MODIFIER_CODES.has(code)) return null;
  if (CODE_NAMES[code]) return CODE_NAMES[code]!;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return code.slice(6);
  if (/^F\d{1,2}$/.test(code)) return code.toLowerCase();
  return null;
}

export interface ChordEvent {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** `cmd+ctrl+alt+shift+<key>`; null while only modifiers are held. */
export function chordFromEvent(e: ChordEvent): string | null {
  const key = keyName(e.code);
  if (!key) return null;
  const parts: string[] = [];
  if (e.metaKey) parts.push('cmd');
  if (e.ctrlKey) parts.push('ctrl');
  if (e.altKey) parts.push('alt');
  if (e.shiftKey) parts.push('shift');
  parts.push(key);
  return parts.join('+');
}

/** Normalizes spelling for comparisons (`opt`/`option` → `alt`, `meta`/`super` → `cmd`, order). */
export function normalizeChord(chord: string): string {
  const parts = chord
    .toLowerCase()
    .split('+')
    .map((p) => p.trim())
    .filter(Boolean);
  const key = parts.pop() ?? '';
  const mods = new Set(
    parts.map((p) => (p === 'opt' || p === 'option' ? 'alt' : p === 'meta' || p === 'super' ? 'cmd' : p)),
  );
  const order = ['cmd', 'ctrl', 'alt', 'shift', 'mod'];
  return [...order.filter((m) => mods.has(m)), key].join('+');
}

export function defaultChords(meta: ActionMeta, platform: Platform): readonly string[] {
  return platform === 'macos' ? meta.mac : meta.linux;
}

/** Effective chords per action: catalog default overridden by `keys.bindings` (`[]` unbinds). */
export function effectiveBindings(
  overrides: Record<string, readonly string[]> | undefined,
  platform: Platform,
): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const a of ACTIONS) out[a.id] = [...(overrides?.[a.id] ?? defaultChords(a, platform))];
  for (const [id, chords] of Object.entries(overrides ?? {})) if (!(id in out)) out[id] = [...chords];
  return out;
}

export interface BindingConflict {
  chord: string;
  actions: string[];
  reserved: boolean;
}

/** Chords bound to more than one action (the reserved-list check lives in L2's `findConflicts`). */
export function duplicateChords(bindings: Record<string, readonly string[]>): BindingConflict[] {
  const byChord = new Map<string, string[]>();
  for (const [action, chords] of Object.entries(bindings)) {
    for (const c of chords) {
      const k = normalizeChord(c);
      byChord.set(k, [...(byChord.get(k) ?? []), action]);
    }
  }
  return [...byChord.entries()]
    .filter(([, actions]) => actions.length > 1)
    .map(([chord, actions]) => ({ chord, actions, reserved: false }));
}

export interface Snippet {
  id: 'hyprland' | 'sway' | 'gnome' | 'macos';
  label: string;
  lines: string[];
}

/** Compositor / desktop snippets that bind `kelta-ctl toggle` (SPEC §8). */
export const TOGGLE_SNIPPETS: readonly Snippet[] = [
  {
    id: 'hyprland',
    label: 'Hyprland',
    lines: [
      'bind = SUPER, K, exec, kelta-ctl toggle',
      'windowrulev2 = workspace 2, class:^(dev.kelta.Kelta)$',
    ],
  },
  {
    id: 'sway',
    label: 'Sway',
    lines: [
      'bindsym $mod+k exec kelta-ctl toggle',
      'for_window [app_id="dev.kelta.Kelta"] move to workspace 2',
    ],
  },
  {
    id: 'gnome',
    label: 'GNOME',
    lines: [
      'Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → +',
      'Name: Kelta    Command: kelta-ctl toggle    Shortcut: your choice',
    ],
  },
  {
    id: 'macos',
    label: 'macOS',
    lines: ['Shortcuts / Automator Quick Action / Karabiner: run `kelta-ctl toggle`'],
  },
];
