import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, reviews, toasts } from '$lib/stores';

import ReviewsPane from './ReviewsPane.svelte';

beforeEach(() => {
  setTransport(createMockTransport().transport);
  reviews.lists = {};
  layout.byProject = {};
  toasts.clear();
});

function mountReviews() {
  return render(ReviewsPane, {
    props: {
      projectId: 'shop',
      tabId: 't',
      paneId: 'p',
      content: { kind: 'reviews', scope: { kind: 'all' } },
      visible: true,
      focused: true,
    },
  });
}

describe('ReviewsPane linked tickets', () => {
  it('a linked ticket badge opens that ticket without opening the review', async () => {
    mountReviews();
    const badge = await screen.findByRole('button', { name: 'Open SHOP-120' });
    await fireEvent.dblClick(badge); // would open the review if it reached the row
    await fireEvent.click(badge);
    await waitFor(() => {
      const l = layout.get('shop')!;
      expect(findContent(l, (c) => c.kind === 'ticket_detail' && c.ticket.key === 'SHOP-120')).toBeTruthy();
    });
    expect(findContent(layout.get('shop')!, (c) => c.kind === 'review_detail')).toBeFalsy();
  });

  it('only an exact key opens: a key no tracker knows says so', async () => {
    mountReviews();
    // SHOP-150 is linked by a review but no ticket has that key (SHOP-15x do not count).
    await fireEvent.click(await screen.findByRole('button', { name: 'Open SHOP-150' }));
    await waitFor(() => expect(toasts.list.at(-1)?.toast.text).toMatch(/SHOP-150 is not in any/));
    const l = layout.get('shop');
    expect(l && findContent(l, (c) => c.kind === 'ticket_detail')).toBeFalsy();
  });
});
