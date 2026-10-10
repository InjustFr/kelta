// Settings > Tools quick form (Add tool) against the mock transport, plus its pure helpers.
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ToolInfo } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { tools } from '$lib/stores';

import Tools from '../plugins/sections/Tools.svelte';
import { effectiveBindings } from './lib/chords';
import { splitArgv, toolKeyConflict, uniqueToolId } from './lib/tools';

describe('splitArgv', () => {
  it.each([
    ['lazydocker', ['lazydocker']],
    ['  k9s   --context "my ctx" ', ['k9s', '--context', 'my ctx']],
    ["git log --format='%h %s'", ['git', 'log', '--format=%h %s']],
    ['a\\ b "c\\"d" \'e\\f\' ""', ['a b', 'c"d', 'e\\f', '']],
    ['', []],
  ])('%s', (line, argv) => expect(splitArgv(line)).toEqual(argv));

  it('rejects an unclosed quote', () => {
    expect(splitArgv('k9s --context "my ctx')).toBeNull();
    expect(splitArgv("it's")).toBeNull();
  });
});

describe('uniqueToolId', () => {
  it('slugs and suffixes past taken ids', () => {
    expect(uniqueToolId('Lazy Docker', [])).toBe('lazy-docker');
    expect(uniqueToolId('lazygit', ['lazygit', 'lazygit-2'])).toBe('lazygit-3');
  });
});

describe('toolKeyConflict', () => {
  const bindings = effectiveBindings({}, 'linux');
  const others = [{ id: 'lazygit', label: 'lazygit', keybinding: 'ctrl+alt+g' }];
  it('names the action, the reserved list or the other tool', () => {
    expect(toolKeyConflict('ctrl+shift+k', 'x', bindings, others, 'linux')).toMatch(
      /is already "Command palette"; this tool's key will not work/,
    );
    expect(toolKeyConflict('ctrl+space', 'x', bindings, others, 'linux')).toMatch(/reserved/);
    expect(toolKeyConflict('ctrl+alt+g', 'x', bindings, others, 'linux')).toMatch(/"lazygit"/);
    expect(toolKeyConflict('ctrl+alt+g', 'lazygit', bindings, others, 'linux')).toBeNull();
    expect(toolKeyConflict('ctrl+alt+shift+y', 'x', bindings, others, 'linux')).toBeNull();
  });
});

describe('Add tool form', () => {
  afterEach(() => vi.restoreAllMocks());

  async function mount(
    layer: 'global' | 'project' | 'repo',
    edit: (t: ToolInfo[]) => ToolInfo[] = () => [],
  ): Promise<MockControls> {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    controls.state.tools = edit(controls.state.tools);
    await tools.load('shop');
    render(Tools, { layer, projectId: 'shop', repoId: layer === 'repo' ? 'shop-api' : null });
    return controls;
  }

  async function fill(label: string, value: string): Promise<void> {
    await fireEvent.input(await screen.findByLabelText(label), { target: { value } });
  }

  async function add(m: MockControls): Promise<unknown> {
    const button = addButton();
    await vi.waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    await fireEvent.click(button);
    await vi.waitFor(() => expect(m.calls.some((c) => c.cmd === 'settings_set')).toBe(true));
    const sets = m.calls.filter((c) => c.cmd === 'settings_set');
    expect(sets).toHaveLength(1);
    return sets[0]!.args;
  }

  const addButton = (): HTMLButtonElement =>
    within(screen.getByTestId('add-tool')).getByRole('button', { name: 'Add' }) as HTMLButtonElement;

  async function openForm(): Promise<void> {
    await fireEvent.click((await screen.findAllByRole('button', { name: 'Add tool' }))[0]!);
  }

  it('shows the new empty-state copy and writes a terminal tool to the global layer', async () => {
    const m = await mount('global');
    expect(await screen.findByText('Add a terminal program like lazygit, or an app like Fork.')).toBeTruthy();
    expect(
      screen.getByText('Buttons that open programs like lazygit in a pane, or apps like Fork.'),
    ).toBeTruthy();
    await openForm();
    const button = addButton();
    expect(button.disabled).toBe(true);
    await fill('Name', 'lazydocker');
    expect(button.disabled).toBe(true);
    await fill('Command line', 'lazydocker');
    expect(await add(m)).toMatchObject({
      layer: 'global',
      path: 'tools',
      value: [{ id: 'lazydocker', label: 'lazydocker', command: 'lazydocker' }],
    });
  });

  it('keeps Add disabled when the command line has an empty first token', async () => {
    await mount('global');
    await openForm();
    await fill('Name', 'x');
    await fill('Command line', '""');
    expect(addButton().disabled).toBe(true);
  });

  it('splits the command line and rejects an unclosed quote', async () => {
    const m = await mount('global');
    await openForm();
    await fill('Name', 'k9s');
    await fill('Command line', 'k9s --context "my ctx');
    expect(screen.getByRole('alert').textContent).toMatch(/quote is not closed/);
    expect(addButton().disabled).toBe(true);
    await fill('Command line', 'k9s --context "my ctx"');
    expect(((await add(m)) as { value: unknown }).value).toEqual([
      { id: 'k9s', label: 'k9s', command: 'k9s', args: ['--context', 'my ctx'] },
    ]);
  });

  it('suffixes an id taken by a listed tool and leaves that entry alone', async () => {
    const m = await mount('global', (ts) =>
      ts.map((t) => (t.id === 'lazygit' ? { ...t, source: { kind: 'plugin', plugin_id: 'git' } } : t)),
    );
    await openForm();
    await fill('Name', 'lazygit');
    await fill('Command line', 'lazygit');
    const args = (await add(m)) as { layer: string; value: { id: string }[] };
    expect(args.layer).toBe('global');
    expect(args.value.map((t) => t.id)).toEqual(['lazygit-2']);
  });

  it('writes an App as `open -a` on macOS', async () => {
    vi.spyOn(navigator, 'platform', 'get').mockReturnValue('MacIntel');
    const m = await mount('global');
    await openForm();
    await fireEvent.click(screen.getByRole('tab', { name: 'App' }));
    await fill('Name', 'Fork');
    await fill('Application', 'Fork');
    expect(await add(m)).toMatchObject({
      layer: 'global',
      path: 'tools',
      value: [{ id: 'fork', label: 'Fork', kind: 'external', command: 'open', args: ['-a', 'Fork', '.'] }],
    });
  });

  it('has no App option on Linux', async () => {
    await mount('global');
    await openForm();
    expect(screen.getByRole('tab', { name: 'Terminal' })).toBeTruthy();
    expect(screen.queryByRole('tab', { name: 'App' })).toBeNull();
  });

  it('warns when the recorded key is taken, not when it is free', async () => {
    await mount('global');
    await openForm();
    const rec = screen.getByTestId('tool-key');
    await fireEvent.keyDown(rec, { code: 'KeyK', key: 'K', ctrlKey: true, shiftKey: true });
    expect(screen.getByTestId('tool-key-conflict').textContent).toMatch(/Command palette/);
    await fireEvent.keyDown(rec, { code: 'Space', key: ' ', ctrlKey: true });
    expect(screen.getByTestId('tool-key-conflict').textContent).toMatch(/reserved/);
    await fireEvent.keyDown(rec, { code: 'KeyY', key: 'Y', ctrlKey: true, altKey: true, shiftKey: true });
    expect(screen.queryByTestId('tool-key-conflict')).toBeNull();
  });

  it('is absent at the repo layer', async () => {
    await mount('repo');
    await screen.findByText('No tools yet');
    expect(screen.queryByRole('button', { name: 'Add tool' })).toBeNull();
  });

  it('is absent at the project layer, which points to Global', async () => {
    const m = await mount('project');
    await screen.findByText('No tools yet');
    expect(screen.queryByRole('button', { name: 'Add tool' })).toBeNull();
    expect(screen.getByTestId('tools-personal').textContent).toMatch(
      /Tools are personal: they live in your global settings/,
    );
    m.calls.length = 0;
    await fireEvent.click(screen.getByRole('button', { name: 'Go to Global' }));
    await vi.waitFor(() =>
      expect(
        m.calls.some(
          (c) => c.cmd === 'settings_layer_get' && (c.args as { layer: string }).layer === 'global',
        ),
      ).toBe(true),
    );
    expect(await screen.findAllByRole('button', { name: 'Add tool' })).not.toHaveLength(0);
    expect(screen.queryByTestId('tools-personal')).toBeNull();
  });
});
