// Terminal view configuration derived from `[terminal]` settings and the resolved theme.

import type { CursorStyle, OptionAsMeta, Renderer, ShiftEnter, TerminalSettings } from '$lib/gen';

import type { Colors, ThemeMode } from './theme';

export interface TerminalConfig {
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  renderer: Renderer;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  /** xterm scrollback lines (`terminal.view_scrollback`). */
  scrollback: number;
  optionAsMeta: OptionAsMeta;
  copyOnSelect: boolean;
  primarySelection: boolean;
  confirmMultilinePaste: boolean;
  shiftEnter: Record<string, ShiftEnter>;
  minimumContrastRatio: number;
  theme: ThemeMode | Colors;
}

export const DEFAULT_CONFIG: TerminalConfig = {
  fontFamily: 'JetBrains Mono, Menlo, DejaVu Sans Mono, monospace',
  fontSize: 13,
  lineHeight: 1.15,
  letterSpacing: 0,
  renderer: 'auto',
  cursorStyle: 'block',
  cursorBlink: false,
  scrollback: 1000,
  optionAsMeta: 'both',
  copyOnSelect: false,
  primarySelection: true,
  confirmMultilinePaste: true,
  shiftEnter: { claude: 'esc-cr', default: 'passthrough' },
  minimumContrastRatio: 1,
  theme: 'dark',
};

export function configFromSettings(
  t: TerminalSettings | undefined,
  theme: ThemeMode | Colors,
): TerminalConfig {
  if (!t) return { ...DEFAULT_CONFIG, theme };
  return {
    fontFamily: t.font_family,
    fontSize: t.font_size,
    lineHeight: t.line_height,
    letterSpacing: t.letter_spacing,
    renderer: t.renderer,
    cursorStyle: t.cursor_style,
    cursorBlink: t.cursor_blink,
    scrollback: t.view_scrollback,
    optionAsMeta: t.option_as_meta,
    copyOnSelect: t.copy_on_select,
    primarySelection: t.primary_selection,
    confirmMultilinePaste: t.confirm_multiline_paste,
    shiftEnter: { ...t.shift_enter },
    minimumContrastRatio: t.minimum_contrast_ratio,
    theme,
  };
}

/** Normalizes the font family list for CSS (`A, B` → `"A", "B"` is not needed; xterm takes CSS). */
export function fontStack(family: string): string {
  return family
    .split(',')
    .map((f) => f.trim())
    .filter(Boolean)
    .map((f) => (/^["']/.test(f) || /^[a-z-]+$/.test(f) ? f : `"${f}"`))
    .join(', ');
}
