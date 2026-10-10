import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { ToolInfo } from '$lib/gen';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects, sessions, tools, ui } from '$lib/stores';

import '../views/tools/actions';
import ToolStrip from './ToolStrip.svelte';

const LAZYDOCKER = '0199a6b2-0000-7000-8000-000000000004';

/** Shop active, with `edit` applied to the mock tools before they load. */
async function setup(edit: (list: ToolInfo[]) => ToolInfo[] = (l) => l) {
  const created = createMockTransport();
  const m = created.controls;
  setTransport(created.transport);
  m.state.tools = edit(m.state.tools);
  layout.byProject = {};
  tools.byProject = {};
  projects.list = m.state.projects.map((p) => ({ ...p, active: p.id === 'shop' }));
  await Promise.all([sessions.load(), layout.ensure('shop'), tools.load('shop')]);
  return m;
}

const strip = (workItemId: string | null = null) => render(ToolStrip, { projectId: 'shop', workItemId });

describe('ToolStrip', () => {
  it('shows the first 6 tools in config order, labelled, and nothing without tools', async () => {
    const m = await setup((l) => l.slice(0, 2));
    let r = strip();
    expect(r.getAllByRole('button').map((b) => b.textContent?.trim())).toEqual(
      m.state.tools.map((t) => t.label),
    );
    r.unmount();

    const base = m.state.tools[0];
    await setup(() => Array.from({ length: 8 }, (_, i) => ({ ...base, id: `t${i}`, label: `T${i}` })));
    r = strip();
    expect(r.getAllByRole('button').map((b) => b.dataset.toolId)).toEqual([
      't0',
      't1',
      't2',
      't3',
      't4',
      't5',
    ]);
    r.unmount();

    await setup(() => []);
    expect(strip().queryByTestId('tool-strip')).toBeNull();
  });

  it('opens the clicked tool with tools.open, from a focusable native button', async () => {
    const m = await setup();
    const id = m.state.tools[1].id;
    const button = strip().getAllByRole('button')[1];
    // Native buttons in the tab order: Tab reaches them, Enter and Space click them (e2e checks the keys).
    expect(button.tagName).toBe('BUTTON');
    expect(button.tabIndex).toBe(0);
    await fireEvent.click(button);
    await vi.waitFor(() => expect(m.calls.some((c) => c.cmd === 'tool_open')).toBe(true));
    expect(m.calls.find((c) => c.cmd === 'tool_open')!.args).toMatchObject({ tool_id: id });
  });

  it('is pressed while the tool runs in this work item, and clears when its session exits', async () => {
    await setup();
    const { getByRole, unmount } = strip();
    const button = getByRole('button', { name: 'lazydocker' });
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(button.title).toBe('lazydocker - running');
    expect(getByRole('button', { name: 'lazygit' }).getAttribute('aria-pressed')).toBe('false');

    sessions.apply({
      type: 'session.updated',
      session: { ...sessions.get(LAZYDOCKER)!, lifecycle: 'exited' },
    });
    await vi.waitFor(() => expect(button.getAttribute('aria-pressed')).toBe('false'));
    unmount();

    // Another work item has no instance of it.
    sessions.apply({ type: 'session.updated', session: { ...sessions.get(LAZYDOCKER)!, lifecycle: 'live' } });
    expect(strip('W1').getByRole('button', { name: 'lazydocker' }).getAttribute('aria-pressed')).toBe(
      'false',
    );
  });

  it('dims a missing tool and opens the picker with its install hint', async () => {
    const m = await setup((l) =>
      l.map((t) => ({ ...t, installed: t.id === 'lazygit' ? false : t.installed })),
    );
    const button = strip().getByRole('button', { name: 'lazygit' });
    expect(button.classList).toContain('missing');
    expect(button.title).toBe('Not installed');
    m.failNext('tool_open', { code: 'internal', message: 'spawn failed' });
    await fireEvent.click(button);
    await vi.waitFor(() =>
      expect(ui.sheet).toMatchObject({ key: 'tool_picker', props: { query: 'lazygit' } }),
    );
  });

  it('renders the label only for a missing or unknown icon', async () => {
    await setup((l) => l.slice(0, 2).map((t, i) => ({ ...t, icon: i === 0 ? 'no-such-icon' : null })));
    const buttons = strip().getAllByRole('button');
    for (const b of buttons) expect(b.querySelector('svg')).toBeNull();
    expect(buttons[0].textContent?.trim()).toBe('lazydocker');
  });

  it('shows the chord in the tooltip', async () => {
    await setup();
    expect(strip().getByRole('button', { name: 'lazygit' }).title).toMatch(/^lazygit \(.+G\)$/);
  });
});
