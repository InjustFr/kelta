import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { settingsSections, type SettingsSectionId } from '$app/registry';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import schemaJson from '../../../../schema/settings.schema.json';
import Control from './fields/Control.svelte';
import SourceBadge from './fields/SourceBadge.svelte';
import { SettingsEditor, type LayerFile } from './lib/editor.svelte';
import { splitPath } from './lib/paths';
import { fieldKind, leafFields, nodeAt, type FieldKind, type SchemaNode } from './lib/schema';
import { ADVANCED, isAdvanced, SECTION_GROUPS, SECTION_ROOTS, TOML_ROOTS } from './lib/sections';
import SectionForm from './SectionForm.svelte';

const schema = schemaJson as unknown as SchemaNode;

describe('schema -> field kinds', () => {
  const cases: [string, SchemaNode, FieldKind][] = [
    ['bool', { type: 'boolean' }, 'bool'],
    ['int', { type: 'integer' }, 'int'],
    ['float', { type: 'number' }, 'float'],
    ['string', { type: 'string' }, 'string'],
    ['nullable string', { type: ['string', 'null'] }, 'string'],
    ['secret', { type: 'string', 'x-kelta-secret': true }, 'secret'],
    ['enum', { type: 'string', enum: ['a', 'b'] }, 'enum'],
    ['string list', { type: 'array', items: { type: 'string' } }, 'string-list'],
    ['string map', { type: 'object', additionalProperties: { type: 'string' } }, 'string-map'],
    ['keyed list', { type: 'array', items: { type: 'object' }, 'x-kelta-merge': 'by_id' }, 'keyed-list'],
    ['group', { type: 'object', properties: { a: { type: 'string' } } }, 'group'],
  ];
  it.each(cases)('%s', (_n, node, kind) => expect(fieldKind(node)).toBe(kind));

  it('every top-level key of the real schema yields at least one field', () => {
    const roots = Object.keys(schema.properties ?? {});
    expect(roots.length).toBeGreaterThan(10);
    for (const r of roots) expect(leafFields(schema, [r]).length, r).toBeGreaterThan(0);
  });

  it('the ticket sources have their own section', () => {
    expect(Object.keys(schema.properties ?? {})).toContain('sources');
    expect(SECTION_ROOTS.sources).toEqual(['sources', 'polling']);
    expect(SECTION_ROOTS.performance).toEqual(['performance', 'web']);
    expect(Object.values(TOML_ROOTS).flat()).not.toContain('sources');
  });
});

describe('every schema key stays reachable (#282)', () => {
  const formRoots = Object.values(SECTION_ROOTS).flat();
  const tomlRoots = Object.values(TOML_ROOTS).flat();

  it('each top-level key belongs to a section form or is listed as TOML-only (with its Edit TOML link)', () => {
    for (const r of Object.keys(schema.properties ?? {}))
      expect(formRoots.includes(r) || tomlRoots.includes(r), r).toBe(true);
  });

  it('each advanced key exists and is rendered by a section form', () => {
    for (const p of ADVANCED) {
      expect(nodeAt(schema, splitPath(p)), p).toBeTruthy();
      expect(formRoots, p).toContain(splitPath(p)[0]);
    }
  });

  it('every section sits in exactly one nav group', () => {
    const grouped = SECTION_GROUPS.flatMap((g) => g.ids);
    expect(new Set(grouped).size).toBe(grouped.length);
    for (const s of settingsSections) expect(grouped, s.id).toContain(s.id);
  });

  const sections = Object.keys(SECTION_ROOTS) as SettingsSectionId[];
  it.each(sections)('%s renders a field for each key, the advanced ones collapsed', async (sectionId) => {
    const { transport } = createMockTransport();
    setTransport(transport);
    const { container, unmount } = render(SectionForm, { sectionId, layer: 'global' });
    await vi.waitFor(() => expect(container.querySelector('[data-path]')).toBeTruthy());
    const advanced = container.querySelector('[data-testid="advanced"]');
    expect(advanced?.hasAttribute('open') ?? false).toBe(false);
    for (const leaf of leafFields(schema, SECTION_ROOTS[sectionId] ?? [])) {
      // the leaf itself, or the nearest rendered ancestor (a keyed list or a map renders as one field)
      const el = leaf.segs
        .map((_, i) => leaf.segs.slice(0, leaf.segs.length - i).join('.'))
        .map((p) => container.querySelector(`[data-path="${CSS.escape(p)}"]`))
        .find(Boolean);
      expect(el, leaf.path).toBeTruthy();
      const prefixes = leaf.segs.map((_, i) => leaf.segs.slice(0, i + 1).join('.'));
      expect(!!advanced?.contains(el!), leaf.path).toBe(prefixes.some(isAdvanced));
    }
    unmount();
  });
});

describe('Control', () => {
  const mount = (node: SchemaNode, value: unknown) => {
    const onchange = vi.fn();
    render(Control, { node, value: value as never, onchange, label: 'Thing' });
    return onchange;
  };

  it('bool toggles', async () => {
    const onchange = mount({ type: 'boolean' }, false);
    await fireEvent.click(screen.getByRole('switch'));
    expect(onchange).toHaveBeenCalledWith(true);
  });

  it('a number shows the inherited value as its placeholder (#475)', () => {
    render(Control, {
      node: { type: 'number' },
      value: undefined,
      placeholder: '1.15',
      onchange: vi.fn(),
      label: 'Thing',
    });
    const input = screen.getByRole('spinbutton') as HTMLInputElement;
    expect(input.getAttribute('placeholder')).toBe('1.15');
    expect(input.value).toBe('');
  });

  it('a two-value key is a toggle: session host writes daemon / inprocess (#470)', async () => {
    const onchange = vi.fn();
    render(Control, {
      node: nodeAt(schema, ['terminal', 'session_host'])!,
      value: 'daemon',
      path: 'terminal.session_host',
      onchange,
      label: 'Keep sessions running after Kelta quits',
    });
    const sw = screen.getByRole('switch') as HTMLInputElement;
    expect(sw.getAttribute('aria-checked') ?? String(sw.checked)).toBe('true');
    expect(document.querySelector('textarea')).toBeNull();
    await fireEvent.click(sw);
    expect(onchange).toHaveBeenCalledWith('inprocess');
  });

  it('quiet hours: two time fields write HH:MM-HH:MM, empty is off, a half range is an inline error (#472)', async () => {
    const onchange = vi.fn();
    render(Control, {
      node: nodeAt(schema, ['notifications', 'quiet_hours'])!,
      value: '22:00-08:00',
      path: 'notifications.quiet_hours',
      onchange,
      label: 'Do not disturb hours',
    });
    const from = screen.getByLabelText('From') as HTMLInputElement;
    const until = screen.getByLabelText('Until') as HTMLInputElement;
    expect([from.value, until.value]).toEqual(['22:00', '08:00']);
    await fireEvent.input(until, { target: { value: '07:30' } });
    await fireEvent.change(until);
    expect(onchange).toHaveBeenLastCalledWith('22:00-07:30');
    await fireEvent.input(from, { target: { value: '' } });
    await fireEvent.change(from);
    expect(screen.getByRole('alert').textContent).toContain('Enter both times');
    expect(onchange).toHaveBeenCalledTimes(1);
    await fireEvent.input(until, { target: { value: '' } });
    await fireEvent.change(until);
    expect(onchange).toHaveBeenLastCalledWith('');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it.each([
    ['garbage', 'Enter both times'],
    ['10:00-10:00', 'Pick two different times'],
  ])('quiet hours: a bad stored value %s shows the inline error (#472)', (value, error) => {
    render(Control, {
      node: nodeAt(schema, ['notifications', 'quiet_hours'])!,
      value,
      path: 'notifications.quiet_hours',
      onchange: vi.fn(),
      label: 'Do not disturb hours',
    });
    expect(screen.getByRole('alert').textContent).toContain(error);
  });

  it('int commits a number and rejects a fraction', async () => {
    const onchange = mount({ type: 'integer' }, 3);
    const input = screen.getByRole('spinbutton');
    await fireEvent.input(input, { target: { value: '7' } });
    await fireEvent.change(input);
    expect(onchange).toHaveBeenCalledWith(7);
    await fireEvent.input(input, { target: { value: '1.5' } });
    await fireEvent.change(input);
    expect(onchange).toHaveBeenCalledTimes(1);
  });

  it('string commits text', async () => {
    const onchange = mount({ type: 'string' }, 'a');
    const input = screen.getByRole('textbox');
    await fireEvent.input(input, { target: { value: 'abc' } });
    await fireEvent.change(input);
    expect(onchange).toHaveBeenCalledWith('abc');
  });

  it('string list adds a chip on Enter', async () => {
    const onchange = mount({ type: 'array', items: { type: 'string' } }, ['x']);
    const input = screen.getByLabelText('Add to Thing');
    await fireEvent.input(input, { target: { value: 'y' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onchange).toHaveBeenCalledWith(['x', 'y']);
  });

  it('plugin secret setting renders the SecretRef picker', () => {
    // `schema_full` inserts each `plugins.<id>` fragment verbatim.
    const full: SchemaNode = {
      type: 'object',
      properties: {
        plugins: {
          type: 'object',
          properties: {
            qa: { type: 'object', properties: { qa_token: { type: 'string', 'x-kelta-secret': true } } },
          },
        },
      },
    };
    const node = nodeAt(full, ['plugins', 'qa', 'qa_token'])!;
    mount(node, 'keyring:qa');
    expect(screen.getByTestId('secret-control')).toBeTruthy();
    expect(screen.getByText('Set token…')).toBeTruthy();
  });

  it('string map adds an entry', async () => {
    const onchange = mount({ type: 'object', additionalProperties: { type: 'string' } }, { a: '1' });
    expect(screen.getByLabelText('Value of a')).toBeTruthy();
    const key = screen.getByLabelText('New key');
    await fireEvent.input(key, { target: { value: 'b' } });
    await fireEvent.keyDown(key, { key: 'Enter' });
    expect(onchange).toHaveBeenCalled();
    expect(onchange.mock.calls[0]![0]).toMatchObject({ a: '1', b: '' });
  });
});

describe('SettingsEditor: Applies to (#475)', () => {
  const file = (where: string, projectId: string | null, value: object): LayerFile => ({
    layer: projectId ? 'project' : 'global',
    projectId,
    repoId: null,
    where,
    value: value as LayerFile['value'],
    path: '/x.toml',
    trusted: null,
  });

  it('All projects lists the files that set a key to another value; a project inherits it, global-only keys say so', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    const ed = new SettingsEditor();
    await ed.load();
    const here = ed.valueOf('terminal.font_size');
    ed.files = [
      file('All projects', null, { terminal: { font_size: here } }),
      file('Shop', 'shop', { terminal: { font_size: 16 } }),
      file('Billing', 'billing', { terminal: { font_size: here } }),
    ];
    expect(ed.overrides('terminal.font_size').map((o) => [o.file.where, o.value])).toEqual([['Shop', 16]]);
    expect(ed.inherited('terminal.line_height')).toBeUndefined();
    expect(ed.globalOnly('app.language')).toBe(false);

    await ed.target({ layer: 'project', projectId: 'shop' });
    expect(ed.overrides('terminal.font_size')).toEqual([]);
    expect(ed.inherited('terminal.line_height')).toBe(1.15);
    expect(ed.globalOnly('app.language')).toBe(true);
    expect(ed.globalOnly('terminal.font_size')).toBe(false);
    expect(ed.access('app.language').editable).toBe(false);
    await ed.set('terminal.line_height', 1.3);
    expect(ed.inherited('terminal.line_height')).toBeUndefined();
  });
});

describe('SourceBadge', () => {
  it.each(['default', 'plugin', 'global', 'project', 'repo', 'runtime'] as const)('%s', (source) => {
    const { container, unmount } = render(SourceBadge, { source, path: 'a.b' });
    const el = container.querySelector('[data-testid="source-badge"]')!;
    expect(el.getAttribute('data-source')).toBe(source);
    expect(el.textContent?.toLowerCase()).toContain(source === 'global' ? 'all projects' : source);
    unmount();
  });
});
