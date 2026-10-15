import { waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import * as samples from '$lib/gen/fixtures';
import { settings, ticketSteps, tickets, work } from '$lib/stores';
import { idle } from '$lib/stores/loadable';

import { stepOf, workTitle } from './live';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.lists = {};
  tickets.details = {};
  await work.load();
});

describe('workTitle', () => {
  it('fetches a ticket missing from the loaded lists once, then shows its title', async () => {
    const w = (mock.state.work as WorkItem[]).find((i) => i.ticket)!;
    tickets.lists = { other: { ...idle(), data: { items: [] } } } as unknown as typeof tickets.lists;
    workTitle(w);
    workTitle(w);
    await waitFor(() => expect(tickets.details).not.toEqual({}));
    const title = (await tickets.loadDetail(w.ticket!)).data!.ticket.title;
    expect(workTitle(w)).toBe(title);
    // one fetch from workTitle (the second call found it pending) + the one above
    expect(mock.calls.filter((c) => c.cmd === 'tracker_get')).toHaveLength(2);
  });

  it('does not fetch before any list has loaded', async () => {
    const w = (mock.state.work as WorkItem[]).find((i) => i.ticket)!;
    workTitle(w);
    await Promise.resolve();
    expect(mock.calls.some((c) => c.cmd === 'tracker_get')).toBe(false);
  });
});

describe('stepOf (#462)', () => {
  const def = (id: string, label: string) => ({ id, label, enter: [], transition_to: null, comment: null });
  const ref = samples.ticket.ref;

  it('is the stored step on the effective pipeline, null without a row or for a step that left it', () => {
    ticketSteps.byKey = {};
    settings.accept(null, {
      ...samples.effectiveSettings,
      value: {
        ...samples.settingsDefault,
        work: {
          ...samples.settingsDefault.work,
          pipeline: [def('implementing', 'Implementing'), def('pr', 'PR'), def('po', 'PO Review')],
        },
      },
    } as never);
    expect(stepOf(ref)).toBeNull();
    const row = { ticket: ref, source_id: null, step_id: 'po', entered_at: '', by: '' };
    ticketSteps.apply({ type: 'ticket.step', step: row });
    expect(stepOf(ref)).toEqual({ index: 2, steps: ['Implementing', 'PR', 'PO Review'] });
    ticketSteps.apply({ type: 'ticket.step', step: { ...row, step_id: 'gone' } });
    expect(stepOf(ref)).toBeNull();
  });
});
