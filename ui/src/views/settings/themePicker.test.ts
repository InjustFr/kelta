// Settings → General theme picker: keyboard browsing, slot writes, deleting a custom theme.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { settings } from '$lib/stores';

import { confirms } from '../../shell/confirm.svelte';
import ThemePicker from './ThemePicker.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  await ipc.settingsSet({
    layer: 'global',
    path: 'themes.sol',
    value: { name: 'Solarized', base: 'dark', ui: { well: '#002b36', fg: '#93a1a1' }, terminal: {} },
  });
  await settings.load();
});

const grid = (name: string) => screen.getByRole('listbox', { name: new RegExp(name) });
const card = (name: string, id: string) =>
  within(grid(name))
    .getAllByRole('option')
    .find((o) => o.dataset.themeId === id)!;
const writes = () => mock.calls.filter((c) => c.cmd === 'settings_set' || c.cmd === 'settings_reset');

describe('ThemePicker', () => {
  it('previews each theme and marks the slot', () => {
    render(ThemePicker);
    expect(
      within(grid('Dark theme'))
        .getAllByRole('option')
        .map((o) => o.dataset.themeId),
    ).toEqual(['bezel-dark', 'sol']);
    expect(card('Dark theme', 'bezel-dark').getAttribute('aria-selected')).toBe('true');
    expect(card('Dark theme', 'sol').getAttribute('aria-selected')).toBe('false');
    expect((card('Dark theme', 'sol').querySelector('.strip') as HTMLElement).style.background).toBe(
      'rgb(0, 43, 54)',
    );
    expect(within(card('Dark theme', 'bezel-dark')).queryByRole('button')).toBeNull();
    expect(within(card('Dark theme', 'sol')).getByRole('button', { name: 'Delete theme' })).toBeTruthy();
  });

  it('moves focus with arrows without writing, and writes the slot on Enter', async () => {
    render(ThemePicker);
    const before = writes().length;
    card('Dark theme', 'bezel-dark').focus();
    await fireEvent.keyDown(card('Dark theme', 'bezel-dark'), { key: 'ArrowRight' });
    expect(document.activeElement).toBe(card('Dark theme', 'sol'));
    expect(card('Dark theme', 'sol').tabIndex).toBe(0);
    expect(writes()).toHaveLength(before);

    await fireEvent.keyDown(card('Dark theme', 'sol'), { key: 'Enter' });
    await waitFor(() => expect(card('Dark theme', 'sol').getAttribute('aria-selected')).toBe('true'));
    expect(writes().at(-1)).toMatchObject({
      cmd: 'settings_set',
      args: { layer: 'global', path: 'app.dark_theme', value: 'sol' },
    });
  });

  it('deletes a custom theme after confirmation and resets its slot', async () => {
    await ipc.settingsSet({ layer: 'global', path: 'app.dark_theme', value: 'sol' });
    await settings.load();
    render(ThemePicker);
    await fireEvent.keyDown(card('Dark theme', 'sol'), { key: 'Delete' });
    await waitFor(() => expect(confirms.current?.title).toBe('Delete "Solarized"?'));
    confirms.answer('delete');
    await waitFor(() =>
      expect(
        writes()
          .slice(-2)
          .map((c) => [c.cmd, (c.args as { path: string }).path]),
      ).toEqual([
        ['settings_reset', 'themes.sol'],
        ['settings_reset', 'app.dark_theme'],
      ]),
    );
    await waitFor(() => expect(within(grid('Dark theme')).getAllByRole('option')).toHaveLength(1));
    expect(settings.value()?.app.dark_theme).toBe('bezel-dark');
    expect(card('Dark theme', 'bezel-dark').getAttribute('aria-selected')).toBe('true');
  });

  it('shows a hint and no selection for a dangling slot', async () => {
    await ipc.settingsSet({ layer: 'global', path: 'app.light_theme', value: 'sol' });
    await settings.load();
    render(ThemePicker);
    expect(screen.getByTestId('theme-missing').textContent).toBe('Theme "sol" not found, using Bezel');
    expect(within(grid('Light theme')).queryAllByRole('option', { selected: true })).toHaveLength(0);
  });
});
