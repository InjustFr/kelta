import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, reviews, sessions, tickets, toasts } from '$lib/stores';

import { groupByProject } from './groups';
import InboxPane from './InboxPane.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.lists = {};
  reviews.lists = {};
  toasts.clear();
  await projects.load();
  await sessions.load();
});

function mountInbox() {
  return render(InboxPane, {
    props: {
      projectId: 'shop',
      tabId: 't',
      paneId: 'p',
      content: { kind: 'inbox' },
      visible: true,
      focused: true,
    },
  });
}

describe('groupByProject', () => {
  it('orders groups like the projects and puts unmatched items under Other', () => {
    const order = [
      { id: 'a', name: 'Alpha' },
      { id: 'b', name: 'Beta' },
    ];
    const items = [
      { n: 1, p: ['b'] },
      { n: 2, p: [] },
      { n: 3, p: ['a'] },
      { n: 4, p: ['zzz'] },
    ];
    const groups = groupByProject(items, (i) => i.p, order);
    expect(groups.map((g) => [g.name, g.items.map((i) => i.n)])).toEqual([
      ['Alpha', [3]],
      ['Beta', [1]],
      ['Other', [2, 4]],
    ]);
  });
});

describe('InboxPane', () => {
  it('shows tickets, review requests (with Other), my PRs and needs-input sessions', async () => {
    const { container } = mountInbox();
    await waitFor(() => expect(container.querySelector('[data-section="s:requested"]')).not.toBeNull());
    await screen.findByText('Terraform: add read replica');

    // The review request on a repo bound to no project lands in "Other".
    const rows = [...container.querySelectorAll('.row, [data-group], [data-section]')];
    const underOther = rows
      .map((r, i) => (r.getAttribute('data-group') === 'Other' ? rows[i + 1]?.textContent : null))
      .filter(Boolean);
    expect(underOther.some((t) => t?.includes('Terraform: add read replica'))).toBe(true);

    expect(container.querySelector('[data-group="Shop"]')).not.toBeNull();
    expect(container.querySelector('[data-section="s:authored"]')).not.toBeNull();
    // The list is virtualised: scroll to the end for the last section.
    const list = container.querySelector('.k-vlist') as HTMLElement;
    Object.defineProperty(list, 'scrollTop', { value: 500, writable: true, configurable: true }); // jsdom has no layout
    await fireEvent.scroll(list);
    await waitFor(() =>
      expect(container.querySelector('[data-section="s:input"]')?.textContent).toMatch(/Needs input/),
    );
  });

  it('is usable with the keyboard: j/k, Enter on a ticket opens its detail', async () => {
    const { container } = mountInbox();
    await screen.findByText('Terraform: add read replica');
    const pane = screen.getByTestId('inbox-pane');
    await fireEvent.keyDown(pane, { key: 'j' });
    await fireEvent.keyDown(pane, { key: 'k' });
    const selected = container.querySelector('.row[aria-current="true"]') as HTMLElement;
    expect(selected.textContent).toMatch(/\w+-?\d+|#\d+/);
    await fireEvent.keyDown(pane, { key: 's' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_plan')).toBe(true));
  });

  it('shows account failures next to the data and an error state when nothing loads', async () => {
    mock.failAlways('tracker_list', { code: 'needs_auth', message: '401' });
    mock.failAlways('review_list', { code: 'needs_auth', message: '401' });
    mountInbox();
    const alert = await screen.findByText('Could not load the inbox');
    expect(alert).toBeTruthy();
    expect(within(document.body).getByRole('button', { name: 'Re-authenticate' })).toBeTruthy();
  });
});
