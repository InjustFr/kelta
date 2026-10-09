// Project hue as the only accent (DESIGN §2.1): tokens.css clamps --k-project to a fixed oklch L/C.

/** oklch chroma of a `#rgb` / `#rrggbb` colour, or null when it is not a hex colour. */
export function oklchChroma(color: string): number | null {
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
  return Math.hypot(
    1.9779984951 * l - 2.428592205 * md + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * md - 0.808675766 * s,
  );
}

/** The value for --k-project, or null to keep the static accent (no colour, or grey: no hue to borrow). */
export function projectAccent(color: string | null | undefined): string | null {
  if (!color) return null;
  const c = oklchChroma(color);
  return c !== null && c < 0.03 ? null : color;
}
