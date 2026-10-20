// Chord strings (`ctrl+shift+k`, `cmd+alt+left`): capture from a KeyboardEvent (layout character,
// like the key manager) and helpers for the keybinding editor.

import { msg } from '$lib/i18n/index.svelte';
import { ACTIONS, type ActionMeta } from '$lib/gen/actions';
import { eventCode } from '$lib/keys/chords';
import { macKeymap } from '$lib/ui/keymap.svelte';

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
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  getModifierState?: (key: string) => boolean;
}

/** `cmd+ctrl+alt+shift+<key>`; null while only modifiers are held. */
export function chordFromEvent(e: ChordEvent): string | null {
  const key = keyName(eventCode(e));
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
  return macKeymap(platform) ? meta.mac : meta.linux;
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

export type KeyArea = 'app' | 'projects' | 'panes' | 'tickets' | 'terminal' | 'work';

/** Groups of the Keys table, in display order (labels go through `t`). */
export const KEY_AREAS: readonly { id: KeyArea; label: string }[] = [
  { id: 'app', label: msg('App') },
  { id: 'projects', label: msg('Projects') },
  { id: 'panes', label: msg('Tabs & panes') },
  { id: 'tickets', label: msg('Tickets & reviews') },
  { id: 'terminal', label: msg('Terminal') },
  { id: 'work', label: msg('Work') },
];

const AREA_OF: Record<string, KeyArea> = {
  project: 'projects',
  tool: 'projects',
  tab: 'panes',
  pane: 'panes',
  session: 'panes',
  tickets: 'tickets',
  reviews: 'tickets',
  terminal: 'terminal',
  work: 'work',
  editor: 'work',
};

/** Area of an action by the first segment of its id; unknown ids (new or plugin actions) go under App. */
export function actionArea(id: string): KeyArea {
  return AREA_OF[id.split('.')[0]!] ?? 'app';
}

/** Text filter: the trimmed query, any case, inside the label or the id. */
export function matchesText(query: string, label: string, id: string): boolean {
  const q = query.trim().toLowerCase();
  return q === '' || label.toLowerCase().includes(q) || id.toLowerCase().includes(q);
}

/** A press in Press keys mode: its chord, and the keys the manager would try as a prefix key (empty when Cmd/Ctrl/Alt was held). */
export interface KeyPress {
  chord: string;
  keys: readonly string[];
}

/** Prefix-key candidates of a keydown, mirroring `KeyManager.#prefixBinding`: the key, Shift+letter as lowercase, the Digit code. */
export function prefixCandidates(
  e: Pick<KeyboardEvent, 'key' | 'code' | 'shiftKey' | 'metaKey' | 'ctrlKey' | 'altKey'>,
): string[] {
  if (e.metaKey || e.ctrlKey || e.altKey) return [];
  const digit = /^Digit([0-9])$/.exec(e.code)?.[1];
  return [
    e.key,
    ...(e.shiftKey && /^[A-Z]$/.test(e.key) ? [e.key.toLowerCase()] : []),
    ...(digit ? [digit] : []),
  ];
}

/** Whether an action answers a press: one of its chords, or its key after the prefix (candidates as the key manager). */
export function matchesPress(press: KeyPress, chords: readonly string[], prefixKey: string | null): boolean {
  const c = normalizeChord(press.chord);
  return (
    chords.some((x) => normalizeChord(x) === c) || (prefixKey !== null && press.keys.includes(prefixKey))
  );
}

/** Actions other than `except` already bound to `chord`. */
export function actionsOn(
  bindings: Record<string, readonly string[]>,
  chord: string,
  except: string,
): string[] {
  const c = normalizeChord(chord);
  return Object.entries(bindings)
    .filter(([id, chords]) => id !== except && chords.some((x) => normalizeChord(x) === c))
    .map(([id]) => id);
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
      msg('Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → +'),
      msg('Name: Kelta    Command: kelta-ctl toggle    Shortcut: your choice'),
    ],
  },
  {
    id: 'macos',
    label: 'macOS',
    lines: [msg('Shortcuts / Automator Quick Action / Karabiner: run `kelta-ctl toggle`')],
  },
];
