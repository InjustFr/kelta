import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { SettingsSectionId } from '$app/registry';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { settings, ui } from '$lib/stores';

import schemaJson from '../../../../schema/settings.schema.json';
import { COPY, COPY_DONE, SECTION_COPY, type CopyTable } from './copy';
import { joinPath, splitPath } from './lib/paths';
import { childEntries, enumOptions, fieldKind, leafFields, nodeAt, type SchemaNode } from './lib/schema';
import { SECTION_ROOTS } from './lib/sections';
import { takeSourceRequest } from './actions';
import SectionForm from './SectionForm.svelte';
import Worktree from './sections/Worktree.svelte';

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

  it('leafFields orders keys by the table across roots and drops a root of the other OS', () => {
    const paths = () => leafFields(schema, SECTION_ROOTS.worktree!).map((f) => f.path);
    expect(paths().slice(0, 4)).toEqual([
      'work.on_start.assign_me',
      'work.plan_preview',
      'work.finish_first',
      'work.default_template',
    ]);
    const saved = COPY.tickets!; // not inject(): its afterEach would delete the real entry
    COPY.tickets = { ...saved, platform: 'linux' };
    try {
      onOs('MacIntel');
      expect(paths()).not.toContain('tickets.wip_limit');
    } finally {
      COPY.tickets = saved;
    }
  });

  it('Terminal is grouped by task, scrollback outside Advanced (#470)', async () => {
    onOs('MacIntel');
    const { container, unmount } = await renderForm('terminal');
    const heads = [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent);
    expect(heads).toEqual([
      'Text',
      'Cursor',
      'Clipboard',
      'Keyboard',
      'Scrollback & history',
      'Shell',
      'Sound',
    ]);
    for (const p of ['terminal.scrollback', 'terminal.osc52'])
      expect(container.querySelector(`[data-path="${p}"]`)!.closest('[data-testid="advanced"]')).toBeNull();
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
    expect(
      [...container.querySelectorAll('[data-testid="field"]')].map((e) => e.getAttribute('data-path')),
    ).toEqual(expected.map((k) => `window.${k}`));
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

describe('Settings → Work items', () => {
  it('stage links, groups in the order work happens, no ticket move outside Advanced, the note opens Sources → Steps', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await settings.load();
    await import('./actions');
    const seen = vi.fn();
    Element.prototype.scrollIntoView = seen;
    const { container, unmount } = render(Worktree, { layer: 'global', projectId: null, repoId: null });
    await waitFor(() => expect(container.querySelector('[data-path]')).toBeTruthy());
    const paths = (els: Element[]) => els.map((e) => e.getAttribute('data-path'));
    const fields = [...container.querySelectorAll('[data-testid="field"]')];
    expect(paths(fields.filter((f) => !f.closest('[data-testid="advanced"]')))).toEqual([
      'work.on_start.assign_me',
      'work.plan_preview',
      'work.finish_first',
      'work.default_template',
      'worktree.branch_template',
      'worktree.root',
      'worktree.include',
      'worktree.setup',
      'worktree.setup_blocking',
      'worktree.teardown',
      'work.pr.title_template',
      'work.pr.body_template',
      'work.pr.draft',
      'work.pr.merge_method',
      'work.on_merge.offer_cleanup',
      'tickets.wip_limit',
      'work.return_brief_after_mins',
    ]);
    const stages = ['Starting', 'Branch folder', 'Pull request', 'After merge', 'Limits'];
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(stages);
    const nav = within(container.querySelector('[data-testid="work-stages"]') as HTMLElement);
    expect(nav.getAllByRole('button').map((b) => b.textContent?.trim())).toEqual(stages);
    const advanced = container.querySelector('[data-testid="advanced"]')!;
    for (const p of ['work.on_pr.comment', 'work.on_start.transition_to'])
      expect(advanced.querySelector(`[data-path="${p}"]`), p).toBeTruthy();

    await fireEvent.click(nav.getByRole('button', { name: 'After merge' }));
    expect((seen.mock.contexts[0] as HTMLElement).dataset.group).toBe('After merge');

    await fireEvent.click(screen.getByRole('button', { name: 'Edit steps in Sources' }));
    expect(ui.settings).toEqual({ section: 'sources' });
    expect(takeSourceRequest()).toEqual({ id: settings.value()!.sources[0]!.id, steps: true });
    ui.settings = null;
    unmount();
  });

  it('at the repo layer only the Branch folder group shows, with no stage links or Sources note', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await settings.load();
    const { container, unmount } = render(Worktree, { layer: 'repo', projectId: null, repoId: 'r1' });
    await waitFor(() => expect(container.querySelector('[data-path]')).toBeTruthy());
    expect(
      [...container.querySelectorAll('[data-testid="copy-group"] h3')].map((h) => h.textContent),
    ).toEqual(['Branch folder']);
    expect(container.querySelector('[data-testid="work-stages"]')).toBeNull();
    expect(container.querySelector('[data-testid="steps-note"]')).toBeNull();
    unmount();
  });
});
