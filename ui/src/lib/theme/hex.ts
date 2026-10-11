// Hex colour primitives shared by the theme importer and the project accent.

/** `[r, g, b, alpha 0..1]` of `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, or null for anything else. */
export function parseHex(v: unknown): [number, number, number, number] | null {
  const m = typeof v === 'string' ? /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(v.trim()) : null;
  if (!m) return null;
  const h = m[1]!.length <= 4 ? [...m[1]!].map((c) => c + c).join('') : m[1]!;
  const [r, g, b, a = 255] = (h.match(/../g) ?? []).map((x) => parseInt(x, 16));
  return [r!, g!, b!, a / 255];
}

/** sRGB channel 0..255 to linear light 0..1. */
export function linear(v: number): number {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

/** oklab `[L, a, b]` of a parsed colour's sRGB channels. */
export function oklab([r, g, b]: readonly number[]): [number, number, number] {
  const [lr, lg, lb] = [r!, g!, b!].map(linear) as [number, number, number];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}
