// Project colours (DESIGN §2.1): `theme:<slot>` follows the active theme's ANSI colour, a hex stays
// as typed. Resolution needs the theme, so only the UI resolves; the CLI and plugins get the stored value.

import type { ResolvedTheme } from './index';

/** Theme colour slot → ANSI index (1–6, then the bright 9–14). */
export const SLOTS = {
  red: 1,
  green: 2,
  yellow: 3,
  blue: 4,
  magenta: 5,
  cyan: 6,
  'bright-red': 9,
  'bright-green': 10,
  'bright-yellow': 11,
  'bright-blue': 12,
  'bright-magenta': 13,
  'bright-cyan': 14,
} as const;

export type Slot = keyof typeof SLOTS;

/** The Bezel swatches stored as hexes before #253: they were theme picks, so they read as the closest
 *  slot in oklch hue. Slate (grey) has no slot and stays a hex. Read-time only, no config rewrite. */
export const LEGACY_SWATCHES: Readonly<Record<string, Slot>> = {
  '#3e7cb1': 'blue', // harbor
  '#5e8c4a': 'green', // moss
  '#b88a2e': 'yellow', // ochre
  '#b5533c': 'red', // brick
  '#8a5a9e': 'magenta', // plum
  '#2f8c86': 'cyan', // teal
  '#b85278': 'bright-magenta', // rose
  '#4b5fc4': 'bright-blue', // cobalt
  '#85863a': 'bright-yellow', // olive
};

/** "bright-blue" → "Bright blue". */
export function slotLabel(slot: Slot): string {
  const s = slot.replace('-', ' ');
  return s[0]!.toUpperCase() + s.slice(1);
}

export function parseProjectColor(
  stored: string | null | undefined,
): { slot: Slot } | { hex: string } | null {
  if (!stored) return null;
  if (stored.startsWith('theme:')) {
    const slot = stored.slice(6);
    return Object.hasOwn(SLOTS, slot) ? { slot: slot as Slot } : null;
  }
  if (!/^#[0-9a-f]{6}$/i.test(stored)) return null;
  const legacy = LEGACY_SWATCHES[stored.toLowerCase()];
  return legacy ? { slot: legacy } : { hex: stored };
}

/** The colour to paint: the active theme's slot colour, the custom hex as typed, or null. */
export function resolveProjectColor(
  stored: string | null | undefined,
  theme: Pick<ResolvedTheme, 'terminal'>,
): string | null {
  const c = parseProjectColor(stored);
  if (!c) return null;
  return 'slot' in c ? (theme.terminal.ansi[SLOTS[c.slot]] ?? null) : c.hex;
}
