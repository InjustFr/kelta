import { render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { sessions, settings } from '$lib/stores';

import StatusBar from './StatusBar.svelte';

beforeEach(async () => {
  setTransport(createMockTransport().transport);
  await sessions.load();
});

describe('status strip', () => {
  it('has no lamp meter: waiting sessions add no button (the chip and ⌘J are the call signal)', async () => {
    // The mock holds a session waiting on me; the strip still renders no count.
    expect(sessions.all.some((s) => s.attention === 'needs_input')).toBe(true);
    render(StatusBar, { prefixArmed: false });
    expect(screen.queryAllByRole('button')).toHaveLength(0);
    expect(screen.getByTestId('statusbar').textContent).not.toMatch(/need|working|ready to review/);
  });

  it('keeps the exceptions: prefix armed and Restart needed', async () => {
    settings.pendingRestart = ['claude.max_live'];
    render(StatusBar, { prefixArmed: true });
    await waitFor(() => expect(screen.getByTestId('status-prefix')).toBeTruthy());
    expect(screen.getByText('Restart needed')).toBeTruthy();
  });
});
