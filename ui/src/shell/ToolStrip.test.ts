import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, tools } from '$lib/stores';

import '../views/tools/actions';
import ToolStrip from './ToolStrip.svelte';

describe('ToolStrip', () => {
  it('renders project tools in order and opens the clicked one', async () => {
    const mock = createMockTransport();
    setTransport(mock.transport);
    projects.list = mock.controls.state.projects.map((p) => ({ ...p, active: p.id === 'shop' }));
    await tools.load('shop');
    const expected = tools.list('shop');
    const { getAllByRole } = render(ToolStrip, { projectId: 'shop' });
    const buttons = getAllByRole('button');
    expect(buttons.map((b) => b.dataset.toolId)).toEqual(expected.map((t) => t.id));

    await fireEvent.click(buttons[1]);
    await vi.waitFor(() => expect(mock.controls.calls.some((c) => c.cmd === 'tool_open')).toBe(true));
    const call = mock.controls.calls.find((c) => c.cmd === 'tool_open')!;
    expect(JSON.stringify(call.args)).toContain(expected[1].id);
  });
});
