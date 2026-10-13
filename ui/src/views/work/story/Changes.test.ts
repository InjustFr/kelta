import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects, sessions, work } from '$lib/stores';

import Changes from './Changes.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  work.byId = {};
  layout.byProject = {};
  await Promise.all([work.load(), sessions.load(), projects.load()]);
});

const shop155 = (): WorkItem => mock.state.work[3] as WorkItem; // review due, delta in the fixtures
const diffPaths = () =>
  mock.calls.filter((c) => c.cmd === 'work_diff').map((c) => (c.args as { path?: string }).path);

describe('Story › Changes', () => {
  it('lists the delta file by file, lockfiles folded; a click opens the diff at that file', async () => {
    render(Changes, { props: { item: shop155(), open: true } });
    const list = await screen.findByRole('list', { name: 'Changed files' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(10); // 9 files and the folded lockfile row
    expect(rows[0]!.textContent).toMatch(/src\/payments\/retry\.ts\s*\+24 −40/);
    expect(rows[9]!.textContent).toMatch(/1 lockfile or generated file\s*\+1800 −0/);

    await fireEvent.click(within(list).getByText('src/api/orders.ts'));
    await waitFor(() => expect(diffPaths()).toEqual(['src/api/orders.ts']));
    const diff = mock.calls.find((c) => c.cmd === 'work_diff')!.args as { delta?: boolean };
    expect(diff.delta).toBe(true);

    // `]f` from the focused row opens the next file, `[f` the previous one.
    within(list).getByText('src/api/orders.ts').closest('button')!.focus();
    await fireEvent.keyDown(document.activeElement!, { key: ']' });
    await fireEvent.keyDown(document.activeElement!, { key: 'f' });
    await waitFor(() => expect(diffPaths()).toEqual(['src/api/orders.ts', 'src/api/http.ts']));
    await fireEvent.keyDown(document.activeElement!, { key: '[' });
    await fireEvent.keyDown(document.activeElement!, { key: 'f' });
    await waitFor(() => expect(diffPaths()).toHaveLength(3));
    expect(diffPaths()[2]).toBe('src/api/orders.ts');
  });

  it('Mark reviewed clears the item as today; the note field sends through work_send', async () => {
    const item = shop155();
    render(Changes, { props: { item, open: true } });
    await fireEvent.click(await screen.findByRole('button', { name: 'Mark reviewed' }));
    await waitFor(() => expect(work.get(item.id)?.review_due).toBe(false));

    const field = screen.getByRole('textbox', { name: 'Note to Claude' });
    await fireEvent.input(field, { target: { value: 'keep the old signature' } });
    await fireEvent.keyDown(field, { key: 'Enter' });
    await waitFor(() => {
      const send = mock.calls.find((c) => c.cmd === 'work_send');
      expect((send?.args as { prompt?: string } | undefined)?.prompt).toBe('keep the old signature');
    });
  });

  it('while Claude works: Changes so far, no Mark reviewed', async () => {
    mock.state.work[0]!.delta = shop155().delta;
    const busy = { ...(mock.state.work[0] as WorkItem) }; // Claude is working
    work.upsert(busy);
    render(Changes, { props: { item: busy, open: true } });
    await screen.findByRole('list', { name: 'Changed files' });
    expect(screen.getByTestId('story-changes').textContent).toContain('Changes so far');
    expect(screen.queryByRole('button', { name: 'Mark reviewed' })).toBeNull();
  });
});
