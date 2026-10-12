// Every Lucide SVG (aliases included), one chunk each. Loaded on demand by `ensureIcon`, so this map of loaders
// stays out of the main chunk.

const SVGS = import.meta.glob<string>('/node_modules/lucide-static/icons/*.svg', {
  query: '?raw',
  import: 'default',
});

export const svgOf = (name: string): Promise<string> =>
  SVGS[`/node_modules/lucide-static/icons/${name}.svg`]();
