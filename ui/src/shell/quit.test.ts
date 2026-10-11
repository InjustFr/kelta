import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import { confirms } from './confirm.svelte';
import { askQuit } from './quit';

let mock: MockControls;

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
});

const callsOf = (cmd: string) => mock.calls.filter((c) => c.cmd === cmd).map((c) => c.args);

describe('askQuit', () => {
  it('shows the session count and busy sessions; Cancel stays open', async () => {
    const asked = askQuit({ live: 3, busy: ['claude'] });
    await vi.waitFor(() => expect(confirms.current?.body).toContain('3 sessions are running'));
    expect(confirms.current?.details).toEqual(['claude is busy']);
    expect(confirms.current?.actions.map((a) => a.id)).toEqual(['stop', 'keep']);
    confirms.answer(null);
    await asked;
    expect(callsOf('app_quit')).toEqual([]);
  });

  it('keep quits without stopping; nothing is stored unless asked', async () => {
    const asked = askQuit({ live: 1, busy: [] });
    await vi.waitFor(() => expect(confirms.current).not.toBeNull());
    confirms.answer('keep');
    await asked;
    expect(callsOf('app_quit')).toEqual([{ stop: false }]);
    expect(callsOf('settings_set')).toEqual([]);
  });

  it("stop with Don't ask again stores app.quit_sessions then quits stopping", async () => {
    const asked = askQuit({ live: 2, busy: [] });
    await vi.waitFor(() => expect(confirms.current?.checkbox).toBe("Don't ask again"));
    // a second quit while the dialog is open does not queue another one
    await askQuit({ live: 2, busy: [] });
    confirms.checked = true;
    confirms.answer('stop');
    await asked;
    expect(confirms.current).toBeNull();
    expect(callsOf('settings_set')).toEqual([{ layer: 'global', path: 'app.quit_sessions', value: 'stop' }]);
    expect(callsOf('app_quit')).toEqual([{ stop: true }]);
  });
});
