// Small formatting helpers shared by the kit.

import { msg, t } from '$lib/i18n/index.svelte';

const MAC_SYMBOLS: Record<string, string> = {
  cmd: '⌘',
  meta: '⌘',
  super: '⌘',
  ctrl: '⌃',
  alt: '⌥',
  opt: '⌥',
  option: '⌥',
  shift: '⇧',
  enter: '↩',
  return: '↩',
  backspace: '⌫',
  delete: '⌦',
  escape: msg('Esc'),
  esc: msg('Esc'),
  tab: '⇥',
  space: msg('Space'),
  left: '←',
  right: '→',
  up: '↑',
  down: '↓',
  pageup: msg('PgUp'),
  pagedown: msg('PgDn'),
};

const LINUX_NAMES: Record<string, string> = {
  cmd: msg('Super'),
  meta: msg('Super'),
  super: msg('Super'),
  ctrl: msg('Ctrl'),
  alt: msg('Alt'),
  opt: msg('Alt'),
  option: msg('Alt'),
  shift: msg('Shift'),
  enter: msg('Enter'),
  return: msg('Enter'),
  escape: msg('Esc'),
  esc: msg('Esc'),
  space: msg('Space'),
  left: '←',
  right: '→',
  up: '↑',
  down: '↓',
  pageup: msg('PgUp'),
  pagedown: msg('PgDn'),
  backspace: msg('Backspace'),
  delete: msg('Del'),
  tab: msg('Tab'),
};

export function currentPlatform(): 'macos' | 'linux' {
  if (typeof navigator === 'undefined') return 'linux';
  return /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? 'macos' : 'linux';
}

/** Splits a chord (`cmd+shift+k`) into display parts for the platform. `mod` = Cmd / Ctrl. */
export function formatChord(chord: string, platform: 'macos' | 'linux' = currentPlatform()): string[] {
  const table = platform === 'macos' ? MAC_SYMBOLS : LINUX_NAMES;
  // A bare capital (`V`, `P`) is Shift + the letter: lowercasing alone would show `v` and `V` alike.
  const raw = (/^[A-Z]$/.test(chord) ? `shift+${chord}` : chord)
    .toLowerCase()
    .split('+')
    .flatMap((p, i, all) => (p === '' && i === all.length - 1 ? ['+'] : p === '' ? [] : [p]));
  const expanded = raw.flatMap((p) => (p === 'mod' ? [platform === 'macos' ? 'cmd' : 'ctrl'] : [p]));
  return expanded.map((p) =>
    t(table[p] ?? (p.length === 1 ? p.toUpperCase() : p[0]!.toUpperCase() + p.slice(1))),
  );
}

/** "5 min ago" style relative time for stale data labels. */
export function relativeTime(fromMs: number, nowMs: number = Date.now()): string {
  const s = Math.max(0, Math.round((nowMs - fromMs) / 1000));
  if (s < 45) return t('just now');
  const m = Math.round(s / 60);
  if (m < 60) return t('{n} min ago', { n: m });
  const h = Math.round(m / 60);
  if (h < 24) return t('{n} h ago', { n: h });
  return t('{n} d ago', { n: Math.round(h / 24) });
}
