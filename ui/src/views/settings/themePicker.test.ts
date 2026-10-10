// Settings → General theme picker: keyboard browsing, slot writes, deleting and importing a theme.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { settings, toasts } from '$lib/stores';

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
    ).toEqual(['bezel-dark', 'solarized-dark', 'gruvbox-dark', 'nord', 'dracula', 'catppuccin-mocha', 'sol']);
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
    card('Dark theme', 'catppuccin-mocha').focus();
    await fireEvent.keyDown(card('Dark theme', 'catppuccin-mocha'), { key: 'ArrowRight' });
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
    await waitFor(() => expect(within(grid('Dark theme')).getAllByRole('option')).toHaveLength(6));
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

  describe('Import theme...', () => {
    const vscode = JSON.stringify({ name: 'Sol', type: 'dark', colors: { 'editor.background': '#002b36' } });
    async function pick(text: string, name = 'sol.json') {
      const input = screen.getByTestId('theme-import-input') as HTMLInputElement;
      Object.defineProperty(input, 'files', { value: [new File([text], name)], configurable: true });
      await fireEvent.change(input);
    }
    const lastToast = () => toasts.list.at(-1)?.toast.text;

    it('saves the full theme, selects it in its slot and leaves app.theme alone', async () => {
      render(ThemePicker);
      await pick(JSON.stringify({ name: 'Night Owl', colors: { 'editor.background': '#011627' } }));
      await waitFor(() => expect(lastToast()).toBe('Imported Night Owl (used in dark mode)'));
      const [theme, slot] = writes().slice(-2);
      expect(theme).toMatchObject({
        cmd: 'settings_set',
        args: { layer: 'global', path: 'themes.night-owl' },
      });
      const def = (theme!.args as { value: { ui: Record<string, string>; terminal: { ansi: string[] } } })
        .value;
      expect(Object.values(def.ui)).toHaveLength(15);
      expect(def.terminal.ansi).toHaveLength(16);
      expect(slot).toMatchObject({ args: { path: 'app.dark_theme', value: 'night-owl' } });
      expect(writes().some((c) => (c.args as { path: string }).path === 'app.theme')).toBe(false);
      await waitFor(() => expect(card('Dark theme', 'night-owl').getAttribute('aria-selected')).toBe('true'));
    });

    it('asks before replacing a user theme with the same id', async () => {
      render(ThemePicker);
      const before = writes().length;
      await pick(vscode);
      await waitFor(() => expect(confirms.current?.title).toBe('Replace "Solarized"?'));
      confirms.answer(null);
      await waitFor(() => expect(confirms.current).toBeNull());
      expect(writes()).toHaveLength(before);

      await pick(vscode);
      await waitFor(() => expect(confirms.current?.title).toBe('Replace "Solarized"?'));
      confirms.answer('replace');
      await waitFor(() =>
        expect(writes().at(-1)).toMatchObject({ args: { path: 'app.dark_theme', value: 'sol' } }),
      );
      expect(settings.value()?.themes.sol?.name).toBe('Sol');
    });

    it('shows a specific error and writes nothing for a bad file', async () => {
      render(ThemePicker);
      const before = writes().length;
      await pick('{"include": "./dark_plus.json"}');
      await waitFor(() =>
        expect(lastToast()).toMatch(/^Could not import the theme: This theme extends \.\/dark_plus\.json\./),
      );
      await pick('x'.repeat(5 * 1024 * 1024 + 1));
      await waitFor(() =>
        expect(lastToast()).toBe('Could not import the theme: The file is larger than 5 MB'),
      );
      expect(writes()).toHaveLength(before);
    });

    it('reports a failed config write', async () => {
      mock.failNext('settings_set', { code: 'internal', message: 'disk full' });
      render(ThemePicker);
      await pick(JSON.stringify({ name: 'Night Owl', colors: { 'editor.background': '#011627' } }));
      await waitFor(() => expect(lastToast()).toBe('Could not import the theme: disk full'));
      expect(settings.value()?.themes['night-owl']).toBeUndefined();
    });
  });
});
