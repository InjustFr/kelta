// Small formatting helpers shared by the kit.

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
  escape: 'Esc',
  esc: 'Esc',
  tab: '⇥',
  space: 'Space',
  left: '←',
  right: '→',
  up: '↑',
  down: '↓',
  pageup: 'PgUp',
  pagedown: 'PgDn',
};

const LINUX_NAMES: Record<string, string> = {
  cmd: 'Super',
  meta: 'Super',
  super: 'Super',
  ctrl: 'Ctrl',
  alt: 'Alt',
  opt: 'Alt',
  option: 'Alt',
  shift: 'Shift',
  enter: 'Enter',
  return: 'Enter',
  escape: 'Esc',
  esc: 'Esc',
  space: 'Space',
  left: '←',
  right: '→',
  up: '↑',
  down: '↓',
  pageup: 'PgUp',
  pagedown: 'PgDn',
  backspace: 'Backspace',
  delete: 'Del',
  tab: 'Tab',
};

export function currentPlatform(): 'macos' | 'linux' {
  if (typeof navigator === 'undefined') return 'linux';
  return /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent) ? 'macos' : 'linux';
}

/** Splits a chord (`cmd+shift+k`) into display parts for the platform. `mod` = Cmd / Ctrl+Shift. */
export function formatChord(chord: string, platform: 'macos' | 'linux' = currentPlatform()): string[] {
  const table = platform === 'macos' ? MAC_SYMBOLS : LINUX_NAMES;
  const raw = chord
    .toLowerCase()
    .split('+')
    .flatMap((p, i, all) => (p === '' && i === all.length - 1 ? ['+'] : p === '' ? [] : [p]));
  const expanded = raw.flatMap((p) =>
    p === 'mod' ? (platform === 'macos' ? ['cmd'] : ['ctrl', 'shift']) : [p],
  );
  return expanded.map(
    (p) => table[p] ?? (p.length === 1 ? p.toUpperCase() : p[0]!.toUpperCase() + p.slice(1)),
  );
}

/** "5 min ago" style relative time for stale data labels. */
export function relativeTime(fromMs: number, nowMs: number = Date.now()): string {
  const s = Math.max(0, Math.round((nowMs - fromMs) / 1000));
  if (s < 45) return 'just now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}
