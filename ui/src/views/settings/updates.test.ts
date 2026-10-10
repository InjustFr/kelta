// Settings → General updates block and the restart banner (#74).
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { UpdateState } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import UpdateBanner from '../../shell/UpdateBanner.svelte';
import { updates } from '../../shell/updates.svelte';
import Updates from './Updates.svelte';

let mock: MockControls;

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  updates.status = null;
  updates.dismissed = null;
});

const shows = async (state: UpdateState, text: string | RegExp): Promise<void> => {
  mock.state.update.state = state;
  const { unmount } = render(Updates);
  await waitFor(() => expect(screen.getByTestId('update-state').textContent).toMatch(text));
  unmount();
};

describe('Updates block', () => {
  it('shows the running version, its channel and each state', async () => {
    mock.state.update.checked_at = null;
    await shows({ type: 'idle' }, 'Not checked yet');
    expect(screen.queryByTestId('updates')).toBeNull();
    mock.state.update.checked_at = '2026-10-11T08:42:00Z';
    await shows({ type: 'up_to_date' }, /^Last checked .+, up to date$/);
    await shows({ type: 'checking' }, 'Checking…');
    await shows({ type: 'downloading' }, 'Downloading…');
    await shows({ type: 'ready', version: '0.1.0-nightly.2' }, 'Restart to use 0.1.0-nightly.2');
    await shows(
      { type: 'failed', message: 'signature did not verify' },
      'Last check failed: signature did not verify',
    );
    await shows(
      { type: 'unsupported', reason: 'Updates come from your package manager.' },
      'package manager',
    );

    render(Updates);
    await waitFor(() => expect(screen.getByTestId('updates').textContent).toContain('(nightly)'));
    expect(screen.getByTestId('updates').textContent).toContain('Kelta 0.1.0-nightly.202610101230');
    expect(screen.queryByTestId('update-check')).toBeNull(); // unsupported: nothing to check
  });

  it('Check now calls app_update_check and shows its result', async () => {
    render(Updates);
    await waitFor(() => screen.getByTestId('update-check'));
    mock.state.update.state = { type: 'failed', message: 'offline' };
    await fireEvent.click(screen.getByTestId('update-check'));
    await waitFor(() =>
      expect(screen.getByTestId('update-state').textContent).toBe('Last check failed: offline'),
    );
    expect(mock.calls.filter((c) => c.cmd === 'app_update_check')).toHaveLength(1);
  });
});

describe('UpdateBanner', () => {
  it('offers Restart once an update is installed; Later hides it for this version', async () => {
    render(UpdateBanner);
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'app_update_status')).toBe(true));
    expect(screen.queryByTestId('update-banner')).toBeNull();

    mock.state.update.state = { type: 'ready', version: '0.1.0-nightly.2' };
    window.dispatchEvent(new Event('focus'));
    await waitFor(() =>
      expect(screen.getByTestId('update-banner').textContent).toContain(
        'Kelta 0.1.0-nightly.2 is installed. Restart to use it.',
      ),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Later' }));
    expect(screen.queryByTestId('update-banner')).toBeNull();

    // A newer build installed later in the session shows again.
    mock.state.update.state = { type: 'ready', version: '0.1.0-nightly.3' };
    window.dispatchEvent(new Event('focus'));
    await waitFor(() => screen.getByTestId('update-banner'));
  });
});
