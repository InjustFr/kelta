import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { PrLink, WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { tickets, work } from '$lib/stores';

import Ticket from './Ticket.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.details = {};
  work.byId = {};
  await work.load();
});

const pr = (number: number, state: PrLink['state']): PrLink => ({
  url: `https://github.com/acme/shop-api/pull/${number}`,
  account: 'github-work',
  repo: 'acme/shop-api',
  number,
  title: `Part ${number}`,
  branch: `feat/part-${number}`,
  state,
  draft: false,
  ci: 'none',
  review: null,
  source: 'key_match',
});

describe('Story > Ticket', () => {
  it('lists every PR of a ticket worked in several items, each with its state (#456)', async () => {
    const w = mock.state.work.find((x) => x.ticket) as WorkItem;
    const row = mock.state.tickets.find((i) => i.ticket.ref.key === w.ticket!.key)!;
    row.prs = [pr(91, 'merged'), pr(92, 'open')];
    render(Ticket, { props: { item: w, open: true } });
    const list = await screen.findByTestId('story-ticket-prs');
    expect(list.textContent).toMatch(/#91\s*Part 91\s*Merged/);
    expect(list.textContent).toMatch(/#92\s*Part 92\s*Review required/);
  });
});
