import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import InstallSheet from './InstallSheet.svelte';

// The system picker (tauri-plugin-dialog) and the home folder; jsdom has no Tauri.
const open = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));
vi.mock('@tauri-apps/api/path', () => ({ homeDir: () => Promise.resolve('/home/me') }));

const field = () => screen.getByLabelText('Source') as HTMLInputElement;

beforeEach(() => {
  open.mockReset();
  setTransport(createMockTransport().transport);
});

describe('install sheet Browse…', () => {
  it('fills the field with the picked folder, starting at home when the field is empty', async () => {
    open.mockResolvedValue('/home/me/code/my-plugin');
    render(InstallSheet, { onclose: () => {} });
    await fireEvent.click(screen.getByRole('button', { name: 'Browse…' }));
    await waitFor(() => expect(field().value).toBe('/home/me/code/my-plugin'));
    expect(open).toHaveBeenCalledWith({ directory: true, defaultPath: '/home/me', filters: undefined });
  });

  it('starts at the typed path when it exists (~ expanded), and Cancel leaves the field alone', async () => {
    open.mockResolvedValue(null);
    render(InstallSheet, { onclose: () => {} });
    await fireEvent.input(field(), { target: { value: '~/code/x' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Browse…' }));
    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(open.mock.calls[0]?.[0]).toMatchObject({ defaultPath: '/home/me/code/x' });
    expect(field().value).toBe('~/code/x');
  });

  it('Archive… picks a .tgz file', async () => {
    open.mockResolvedValue('/home/me/p.tgz');
    render(InstallSheet, { onclose: () => {} });
    await fireEvent.click(screen.getByRole('button', { name: 'Archive…' }));
    await waitFor(() => expect(field().value).toBe('/home/me/p.tgz'));
    expect(open.mock.calls[0]?.[0]).toMatchObject({
      directory: false,
      filters: [{ extensions: ['tgz', 'gz'] }],
    });
  });
});
