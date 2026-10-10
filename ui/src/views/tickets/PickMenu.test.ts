import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import PickMenu from './PickMenu.svelte';

describe('PickMenu', () => {
  it('loads once while the filter stays idle', async () => {
    const load = vi.fn(async () => [{ id: 'a', label: 'Alice' }]);
    render(PickMenu, {
      props: {
        label: 'Assign',
        placeholder: 'Filter people',
        x: 0,
        y: 0,
        load,
        onselect: vi.fn(),
        onclose: vi.fn(),
      },
    });
    expect(await screen.findByRole('menuitem', { name: /Alice/ })).toBeTruthy();
    await new Promise((r) => setTimeout(r, 500));
    expect(load).toHaveBeenCalledTimes(1);
  });
});
