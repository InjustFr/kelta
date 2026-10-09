import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { dispatch } from '$lib/actions';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, tools, ui } from '$lib/stores';

import './actions';
import ToolPicker from './ToolPicker.svelte';

function setup() {
  const mock = createMockTransport();
  setTransport(mock.transport);
  return mock.controls;
}

describe('missing tools', () => {
  it('picker checks a never-checked tool before opening and shows its install hint', async () => {
    const m = setup();
    await tools.load('shop');
    const t = tools.list('shop')[0];
    t.installed = null; // never checked by the backend
    m.state.tools.find((x) => x.id === t.id)!.installed = false;
    const { getByText, findByText, getAllByText } = render(ToolPicker, { onclose: vi.fn(), projectId: 'shop' });
    await fireEvent.click(getByText(t.label, { selector: '.label' }));
    expect(await findByText(/brew install/)).toBeTruthy();
    expect(getAllByText('Check again').length).toBeGreaterThan(0);
    expect(m.calls.some((c) => c.cmd === 'tool_open')).toBe(false);
  });

  it('tools.open with a missing tool opens the picker with its check instead of opening', async () => {
    const m = setup();
    const id = m.state.tools[0].id;
    m.state.tools[0].installed = false;
    projects.list = m.state.projects.map((p) => ({ ...p, active: p.id === 'shop' }));
    await dispatch('tools.open', { tool_id: id });
    expect(m.calls.some((c) => c.cmd === 'tool_open')).toBe(false);
    expect(ui.sheet).toMatchObject({ key: 'tool_picker', props: { query: id, checks: { [id]: { installed: false } } } });
  });
});
