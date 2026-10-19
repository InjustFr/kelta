import { describe, expect, it } from 'vitest';

import { settingsDefault } from '$lib/gen/fixtures';

import { configFromSettings, fontStack } from './config';

describe('configFromSettings', () => {
  it('appends the bundled Kelta Symbols fallback after the user font family', () => {
    const cfg = configFromSettings(
      { ...settingsDefault.terminal, font_family: 'Fira Code, monospace' },
      'dark',
    );
    expect(cfg.fontFamily).toBe('Fira Code, monospace, Kelta Symbols');
    expect(fontStack(cfg.fontFamily)).toBe('"Fira Code", monospace, "Kelta Symbols"');
  });

  it('appends it to the default stack without settings', () => {
    expect(configFromSettings(undefined, 'dark').fontFamily).toBe(
      'JetBrains Mono, Menlo, DejaVu Sans Mono, monospace, Kelta Symbols',
    );
  });
});
