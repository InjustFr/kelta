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
