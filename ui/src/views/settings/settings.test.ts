import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import schemaJson from '../../../../schema/settings.schema.json';
import Control from './fields/Control.svelte';
import SourceBadge from './fields/SourceBadge.svelte';
import { fieldKind, leafFields, type FieldKind, type SchemaNode } from './lib/schema';

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

describe('SourceBadge', () => {
  it.each(['default', 'plugin', 'global', 'project', 'repo', 'runtime'] as const)('%s', (source) => {
    const { container, unmount } = render(SourceBadge, { source, path: 'a.b' });
    const el = container.querySelector('[data-testid="source-badge"]')!;
    expect(el.getAttribute('data-source')).toBe(source);
    expect(el.textContent?.toLowerCase()).toContain(source);
    unmount();
  });
});
