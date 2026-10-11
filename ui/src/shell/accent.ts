// Project hue as the only accent (DESIGN §2.1): tokens.css clamps --k-project to a fixed oklch L/C.

import { oklab, parseHex } from '$lib/theme/hex';

/** oklch chroma and hue (degrees) of a `#rgb` / `#rrggbb` colour, or null when it is not a hex colour. */
export function oklch(color: string): { c: number; h: number } | null {
  const t = color.trim();
  // no alpha forms: those keep the colour as is in projectAccent
  const p = t.length === 4 || t.length === 7 ? parseHex(t) : null;
  if (!p) return null;
  const [, a, bb] = oklab(p);
  return { c: Math.hypot(a, bb), h: ((Math.atan2(bb, a) * 180) / Math.PI + 360) % 360 };
}

/** oklch hue of --k-lamp-needs-input (#c93a33 light, #e5534b dark, both ~27°). */
const ALARM_HUE = 27;

/** The value for --k-project, or null to keep the static accent: no colour, an unparseable one
 *  (it would invalidate --k-accent and every focus ring), grey (no hue to borrow), or red
 *  (a red accent makes every selected row look like a needs-input lamp, DESIGN rule 2). */
export function projectAccent(color: string | null | undefined): string | null {
  if (!color || !globalThis.CSS?.supports('color', color)) return null;
  // shortcut: named / rgb() colours skip the grey and red checks; parse them if users hit it.
  const o = oklch(color);
  if (!o) return color;
  const fromAlarm = Math.abs(((o.h - ALARM_HUE + 540) % 360) - 180);
  return o.c < 0.03 || fromAlarm < 30 ? null : color;
}
