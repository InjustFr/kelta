import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { TicketItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import ActionBar from './ActionBar.svelte';

let mock: MockControls;
const item = (key: string): TicketItem =>
  structuredClone(mock.state.tickets.find((i) => i.ticket.ref.key === key)!);
const bar = (it: TicketItem) => {
  render(ActionBar, { props: { item: it, branch: null, resume: false, onrun: () => {} } });
  return within(screen.getByRole('toolbar', { name: 'Ticket actions' }));
};
const row = (b: ReturnType<typeof bar>) =>
  b.getAllByRole('button').map((x) => x.getAttribute('aria-label') ?? x.textContent?.trim());

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
});

describe('ActionBar', () => {
  it('without a PR: Start work is the primary, then Move and Comment with their keys; Assign to me is under More', async () => {
    const b = bar(item('SHOP-151')); // no PR
    expect(row(b)).toEqual(['Start work', 'Move', 'Comment', 'More']);
    expect(b.getByRole('button', { name: 'Start work' }).classList.contains('primary')).toBe(true);
    expect(b.getByRole('button', { name: 'Move' }).classList.contains('primary')).toBe(false);
    expect(
      ['Start work', 'Move', 'Comment'].map(
        (n) => b.getByRole('button', { name: n }).querySelector('kbd')?.textContent,
      ),
    ).toEqual(['S', 'M', 'C']);
    await fireEvent.click(b.getByRole('button', { name: 'More' }));
    expect(screen.getByRole('menuitem', { name: /^Assign to me/ })).toBeTruthy();
  });

  it('with a PR: Open PR joins the row', () => {
    expect(row(bar(item('SHOP-142')))).toEqual(['Start work', 'Move', 'Open PRs', 'Comment', 'More']);
  });
});
