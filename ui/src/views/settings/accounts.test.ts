// Accounts wizard: "Sign in with GitHub" (device flow) against the mock transport.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import { oauthHost, oauthSecretRef, emptyDraft } from './lib/accounts';
import Accounts from './sections/Accounts.svelte';

async function wizardAtToken(clientIds: Record<string, string>): Promise<MockControls> {
  const { transport, controls } = createMockTransport();
  setTransport(transport);
  await ipc.settingsSet({ layer: 'global', path: 'oauth.client_ids', value: clientIds });
  render(Accounts, { layer: 'global', projectId: null, repoId: null });
  await fireEvent.click(await screen.findByTestId('add-account'));
  await fireEvent.click(document.querySelector('[data-kind="github"]')!);
  await fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  return controls;
}

describe('browser sign-in', () => {
  it('shows the code, then saves an oauth account and tests it', async () => {
    const controls = await wizardAtToken({ 'github.com': 'Ov23liTEST' });
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in with GitHub' }));

    await vi.waitFor(() => expect(controls.calls.some((c) => c.cmd === 'account_test')).toBe(true));
    const start = controls.calls.find((c) => c.cmd === 'oauth_device_start');
    expect(start?.args).toEqual({
      kind: 'github',
      base_url: 'https://api.github.com',
      secret_ref: 'keyring:github-github',
    });
    expect(controls.calls.find((c) => c.cmd === 'oauth_device_finish')?.args).toEqual({
      user_code: 'WDJB-MJHT',
    });
    const saved = controls.calls.find(
      (c) =>
        c.cmd === 'settings_set' &&
        (c.args as { path?: string } | undefined)?.path === 'accounts.github-github',
    );
    expect((saved?.args as { value?: unknown } | undefined)?.value).toEqual({
      kind: 'github',
      auth: 'oauth',
      secret: 'keyring:github-github',
    });
    expect(document.querySelector('[data-testid="account-wizard"]')?.getAttribute('data-step')).toBe('3');
  });

  it('shows the user code with copy and open while waiting', async () => {
    const { transport, controls } = createMockTransport();
    let release: () => void = () => {};
    setTransport({
      ...transport,
      invoke: <T>(cmd: string, args?: Record<string, unknown> | Uint8Array) =>
        cmd === 'oauth_device_finish'
          ? new Promise<T>((r) => (release = () => r(null as T)))
          : transport.invoke<T>(cmd, args),
    });
    await ipc.settingsSet({ layer: 'global', path: 'oauth.client_ids', value: { 'github.com': 'x' } });
    render(Accounts, { layer: 'global', projectId: null, repoId: null });
    await fireEvent.click(await screen.findByTestId('add-account'));
    await fireEvent.click(document.querySelector('[data-kind="github"]')!);
    await fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in with GitHub' }));

    expect((await screen.findByTestId('user-code')).textContent).toBe('WDJB-MJHT');
    await fireEvent.click(screen.getByRole('button', { name: 'Copy code' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Open GitHub' }));
    expect(controls.calls.find((c) => c.cmd === 'clipboard_write')?.args).toEqual({
      kind: 'clipboard',
      text: 'WDJB-MJHT',
    });
    expect(controls.calls.find((c) => c.cmd === 'open_external')?.args).toEqual({
      url: 'https://github.com/login/device',
    });
    release();
  });

  it('reports a denied sign-in and offers to retry', async () => {
    const controls = await wizardAtToken({ 'github.com': 'x' });
    controls.failNext('oauth_device_finish', {
      code: 'cancelled',
      message: 'sign-in was denied in the browser',
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Sign in with GitHub' }));
    expect((await screen.findByTestId('device-sign-in-error')).textContent).toContain('denied');
    expect(screen.getByRole('button', { name: 'Sign in with GitHub' })).toBeTruthy();
    expect(
      controls.calls.some(
        (c) =>
          c.cmd === 'settings_set' &&
          (c.args as { path?: string } | undefined)?.path === 'accounts.github-github',
      ),
    ).toBe(false);
  });

  it('hides the button and explains registration without a client id', async () => {
    await wizardAtToken({});
    expect(await screen.findByTestId('oauth-unset')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Sign in with GitHub' })).toBeNull();
  });

  it('keys client ids by web host and reuses keyring/file refs', () => {
    expect(oauthHost('github', 'https://api.github.com')).toBe('github.com');
    expect(oauthHost('github', 'https://ghe.acme.example/api/v3')).toBe('ghe.acme.example');
    expect(oauthHost('gitlab', '')).toBe('gitlab.com');
    expect(oauthHost('jira', 'https://x.atlassian.net')).toBeNull();
    const d = { ...emptyDraft('gitlab'), id: 'gl' };
    expect(oauthSecretRef(d)).toBe('keyring:gl');
    expect(oauthSecretRef({ ...d, secret: 'file:gl-work' })).toBe('file:gl-work');
  });
});
