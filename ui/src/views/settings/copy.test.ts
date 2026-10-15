import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { SettingsSectionId } from '$app/registry';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import schemaJson from '../../../../schema/settings.schema.json';
import { COPY, COPY_DONE, SECTION_COPY, type CopyTable } from './copy';
import { joinPath, splitPath } from './lib/paths';
import { childEntries, enumOptions, fieldKind, leafFields, nodeAt, type SchemaNode } from './lib/schema';
import { SECTION_ROOTS } from './lib/sections';
import SectionForm from './SectionForm.svelte';

const schema = schemaJson as unknown as SchemaNode;
const onOs = (p: 'MacIntel' | 'Linux x86_64') => vi.spyOn(navigator, 'platform', 'get').mockReturnValue(p);

const injected: string[] = [];
const inject = (table: CopyTable) => {
  Object.assign(COPY, table);
  injected.push(...Object.keys(table));
};
afterEach(() => {
  vi.restoreAllMocks();
  for (const k of injected.splice(0)) delete COPY[k];
});

async function renderForm(sectionId: SettingsSectionId) {
  const { transport } = createMockTransport();
  setTransport(transport);
  const r = render(SectionForm, { sectionId, layer: 'global' });
  await vi.waitFor(() => expect(r.container.querySelector('[data-path]')).toBeTruthy());
  return r;
}

describe('settings copy table', () => {
  it('each entry names a key of its own section', () => {
    for (const [section, table] of Object.entries(SECTION_COPY)) {
      for (const path of Object.keys(table)) {
        expect(nodeAt(schema, splitPath(path)), path).toBeTruthy();
        expect(SECTION_ROOTS[section as SettingsSectionId], path).toContain(splitPath(path)[0]);
      }
    }
  });

  it('option labels name real enum values', () => {
    for (const [path, entry] of Object.entries(COPY)) {
      if (!entry.options) continue;
      const values = enumOptions(nodeAt(schema, splitPath(path))!).map((o) => o.value);
      for (const v of Object.keys(entry.options)) expect(values, `${path}.${v}`).toContain(v);
    }
  });

  it('enum options take their label from the table', () => {
    const labels = enumOptions(nodeAt(schema, ['keys', 'linux_terminal'])!, 'keys.linux_terminal').map(
      (o) => o.label,
    );
    expect(labels).toEqual(['Terminal first', 'Kelta first']);
  });

  it('platform hides the keys of the other OS', () => {
    const paths = () => leafFields(schema, ['terminal', 'keys']).map((f) => f.path);
    onOs('MacIntel');
    expect(paths()).toContain('terminal.option_as_meta');
    expect(paths()).not.toContain('terminal.primary_selection');
    expect(paths()).not.toContain('keys.linux_terminal');
    onOs('Linux x86_64');
    expect(paths()).not.toContain('terminal.option_as_meta');
    expect(paths()).toContain('terminal.primary_selection');
    expect(paths()).toContain('keys.linux_terminal');
  });

  it('the form leaves out the keys of the other OS', async () => {
    onOs('MacIntel');
    const { container, unmount } = await renderForm('terminal');
    expect(container.querySelector('[data-path="terminal.primary_selection"]')).toBeNull();
    expect(container.querySelector('[data-path="terminal.option_as_meta"]')).toBeTruthy();
    unmount();
  });

  it('fields follow the table order under group headings, the rest A→Z', async () => {
    inject({
      'window.restore_geometry': { order: 0, group: 'On launch' },
      'window.decorations': { order: 1, group: 'Look' },
    });
    const expected = ['restore_geometry', 'decorations', 'close_behavior'];
    expect(childEntries(schema.properties!.window!, 'window').map(([k]) => k)).toEqual(expected);
    const { container, unmount } = await renderForm('window');
    expect([...container.querySelectorAll('[data-path]')].map((e) => e.getAttribute('data-path'))).toEqual(
      expected.map((k) => `window.${k}`),
    );
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(['On launch', 'Look']);
    expect(
      container.querySelector('[data-path="window.close_behavior"]')!.closest('[data-testid="copy-group"]'),
    ).toBeNull();
    unmount();
  });

  it('every key of a finished section has a label (COPY_DONE)', () => {
    for (const os of ['MacIntel', 'Linux x86_64'] as const) {
      onOs(os);
      const missing = new Set<string>();
      for (const s of COPY_DONE) {
        const roots = SECTION_ROOTS[s] ?? [];
        // mirrors SectionForm: a lone root group shows its children, not itself
        const min = roots.length === 1 && fieldKind(schema.properties![roots[0]!]!) === 'group' ? 2 : 1;
        for (const leaf of leafFields(schema, roots))
          for (let i = min; i <= leaf.segs.length; i++) {
            const p = joinPath(leaf.segs.slice(0, i));
            if (!COPY[p]?.label) missing.add(p);
          }
      }
      expect([...missing]).toEqual([]);
    }
  });
});
