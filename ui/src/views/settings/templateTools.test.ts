// Insert menu and live example of template settings (#479).
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import Control from './fields/Control.svelte';

const calls: Record<string, unknown>[] = [];

beforeEach(() => {
  calls.length = 0;
  const { transport } = createMockTransport();
  // the core answers `R:<template>`, and refuses a placeholder it does not know
  setTransport({
    ...transport,
    invoke: <T>(cmd: string, args?: Record<string, unknown> | Uint8Array): Promise<T> => {
      if (cmd !== 'template_preview') return transport.invoke<T>(cmd, args);
      const a = args as { path: string; template: string };
      calls.push(a);
      return a.template.includes('{nope}')
        ? Promise.reject(new Error('unknown placeholder {nope}'))
        : Promise.resolve({ text: `R:${a.template}`, git_ok: false } as T);
    },
  });
});

function mount() {
  const onchange = vi.fn();
  render(Control, {
    node: { type: 'string' },
    value: 'feat/-x',
    onchange,
    label: 'Branch name',
    template: true,
    path: 'worktree.branch_template',
    id: 'f-bn',
  });
  return { onchange, input: screen.getByRole('textbox') as HTMLInputElement };
}

describe('template tools', () => {
  it('Insert puts the picked placeholder at the cursor, from the keyboard', async () => {
    const { onchange, input } = mount();
    input.setSelectionRange(5, 5);
    await fireEvent.click(screen.getByRole('button', { name: 'Insert' }));
    const menu = screen.getByRole('menu');
    expect(menu.textContent).toContain('Ticket');
    expect(menu.textContent).toContain('|slug');
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'Enter' });
    expect(onchange).toHaveBeenCalledWith('feat/{ticket.key}-x');
    await waitFor(() => {
      expect(document.activeElement).toBe(input);
      expect(input.selectionStart).toBe(17);
    });
  });

  it("the example is the core's rendering and follows typing", async () => {
    const { input } = mount();
    const example = () => screen.getByTestId('template-example').textContent;
    await waitFor(() => expect(example()).toContain('Example for KEL-214: R:feat/-x'));
    await fireEvent.input(input, { target: { value: '{key}' } });
    await waitFor(() => {
      expect(example()).toContain('R:{key}');
      expect(example()).toContain('Not a valid git branch name');
    });
    expect(calls.at(-1)).toEqual({ path: 'worktree.branch_template', template: '{key}' });
  });

  it('shows why there is no example', async () => {
    const { input } = mount();
    await fireEvent.input(input, { target: { value: '{nope}' } });
    await waitFor(() =>
      expect(screen.getByTestId('template-example').textContent).toContain(
        'No example: unknown placeholder {nope}',
      ),
    );
  });
});
