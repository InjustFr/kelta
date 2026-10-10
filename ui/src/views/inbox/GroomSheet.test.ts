// Grooming pass (#145) against the mock transport: New first, n/N/l triage, j/k skip.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, work } from '$lib/stores';

import GroomSheet from './GroomSheet.svelte';
import { nextUp } from './nextUp.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  await Promise.all([projects.load(), work.load()]);
});

const shown = () => screen.getByTestId('ticket-detail').getAttribute('aria-label');
const press = (key: string) => fireEvent.keyDown(screen.getByTestId('groom'), { key });
const puts = () =>
  mock.calls
    .filter((c) => c.cmd === 'next_up_put')
    .map((c) => (c.args as { item: (typeof mock.state.nextUp.items)[0] }).item);

describe('GroomSheet', () => {
  it('opens on the New ticket; n, N and l triage it, j and k skip', async () => {
    render(GroomSheet, { props: { onclose: vi.fn() } });
    await waitFor(() => expect(shown()).toBe('Ticket 4610'));
    expect(screen.getByText('New')).toBeTruthy();

    await press('n');
    await waitFor(() => expect(shown()).not.toBe('Ticket 4610'));
    expect(puts().at(-1)).toMatchObject({ ticket: { key: '4610' }, rank: 0, snoozed_until: null });
    expect(nextUp.isNew({ account: 'redmine-corp', key: '4610', id: '' })).toBe(false);

    const second = shown();
    await press('j');
    await waitFor(() => expect(shown()).not.toBe(second));
    await press('k');
    await waitFor(() => expect(shown()).toBe(second));

    await press('N');
    await waitFor(() => expect(shown()).not.toBe(second));
    expect(puts().at(-1)!.rank).toBe(-1); // on top of 4610

    const third = shown();
    await press('l');
    await waitFor(() => expect(shown()).not.toBe(third));
    const snooze = puts().at(-1)!;
    expect(snooze.rank).toBeNull();
    const days = (Date.parse(snooze.snoozed_until!) - Date.now()) / 86_400_000;
    expect(Math.round(days)).toBe(7);
  });
});
