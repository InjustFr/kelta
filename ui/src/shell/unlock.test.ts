import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { SecretBackendStatus, Settings } from '$lib/gen';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import Accounts from '../views/settings/sections/Accounts.svelte';

import { prompts } from './confirm.svelte';
import { startupUnlock } from './unlock';

function withFileStatus(detail: string): void {
  const mock = createMockTransport();
  const file: SecretBackendStatus = { backend: 'encrypted-file', available: false, detail };
  setTransport({
    ...mock.transport,
    invoke: <T>(cmd: string, args?: Record<string, unknown> | Uint8Array) =>
      cmd === 'secret_backends_status' ? Promise.resolve([file] as T) : mock.transport.invoke<T>(cmd, args),
  });
}

const withAccounts = (secret: string | null): Settings =>
  ({ accounts: { gh: { kind: 'github', secret } } }) as unknown as Settings;

describe('encrypted secrets file', () => {
  it('prompts once at startup for a locked file used by an account', async () => {
    withFileStatus('locked: enter its passphrase in Settings → Accounts');
    const ask = vi.spyOn(prompts, 'ask').mockResolvedValue(null);
    const once = startupUnlock();
    once(null);
    once(withAccounts('file:gh'));
    once(withAccounts('file:gh'));
    await vi.waitFor(() => expect(ask).toHaveBeenCalledTimes(1));
    ask.mockRestore();
  });

  it('never prompts for a missing file, nor for a file: account added mid-session', async () => {
    withFileStatus('not set up: choose a passphrase to create it');
    const ask = vi.spyOn(prompts, 'ask').mockResolvedValue(null);
    startupUnlock()(withAccounts('file:gh'));
    const later = startupUnlock();
    later(withAccounts(null));
    later(withAccounts('file:gh'));
    await new Promise((r) => setTimeout(r, 20));
    expect(ask).not.toHaveBeenCalled();
    ask.mockRestore();
  });

  it('Settings → Accounts shows the create fields when no file exists', async () => {
    withFileStatus('not set up: choose a passphrase to create it');
    render(Accounts, { layer: 'global', projectId: null, repoId: null });
    expect(await screen.findByLabelText('Repeat the passphrase')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Create' })).toBeTruthy();
  });
});
