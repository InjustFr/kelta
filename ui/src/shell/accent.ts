// Project hue as the only accent (DESIGN §2.1): tokens.css clamps --k-project to a fixed oklch L/C.

/** oklch chroma and hue (degrees) of a `#rgb` / `#rrggbb` colour, or null when it is not a hex colour. */
export function oklch(color: string): { c: number; h: number } | null {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color.trim());
  if (!m) return null;
  const hex = m[1]!.length === 3 ? [...m[1]!].map((c) => c + c).join('') : m[1]!;
  const [r, g, b] = [0, 2, 4].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const md = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const a = 1.9779984951 * l - 2.428592205 * md + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * md - 0.808675766 * s;
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
