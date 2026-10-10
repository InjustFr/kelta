import { fireEvent, render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import { describe, expect, it } from 'vitest';

import { registerAction } from '$lib/actions';

import { hud } from './hud.svelte';
import JumpHud from './JumpHud.svelte';

describe('JumpHud', () => {
  it('Enter on the empty state opens the offered view, and only then', async () => {
    let opened = 0;
    const off = registerAction('test.hud_enter', () => void (opened += 1));
    render(JumpHud);
    hud.show('2/7 · needs input · SHOP-142');
    flushSync();
    expect(screen.getByTestId('jump-hud').textContent).toContain('2/7 · needs input · SHOP-142');
    await fireEvent.keyDown(window, { key: 'Enter' });
    expect(opened).toBe(0);

    hud.show('nothing waiting', { action: 'test.hud_enter', label: 'Tickets' });
    flushSync();
    await fireEvent.keyDown(window, { key: 'Enter' });
    expect(opened).toBe(1);
    flushSync();
    expect(screen.queryByTestId('jump-hud')).toBeNull();
    off();
  });
});
