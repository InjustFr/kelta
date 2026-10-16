import { render, screen, waitFor, fireEvent } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects, sessions, work } from '$lib/stores';

import ReturnBrief from './ReturnBrief.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  work.byId = {};
  work.git = {};
  layout.byProject = {};
  await Promise.all([work.load(), sessions.load(), projects.load(), work.refreshStatus()]);
});

function mountHeader(item: WorkItem) {
  return render(ReturnBrief, { props: { projectId: item.project_id, workItemId: item.id } });
}

const item = (i: number): WorkItem => mock.state.work[i] as WorkItem;

describe('return strip', () => {
  const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

  it('shows the next: note, the delta and the last message after 20 min away; x dismisses', async () => {
    const w = { ...item(3), left_at: ago(45) }; // SHOP-155: note, message and delta in the fixtures
    work.upsert(w);
    mountHeader(w);
    const strip = await screen.findByTestId('return-brief');
    await waitFor(() => expect(document.activeElement).toBe(strip)); // x / v reach it, not the pane
    expect(strip.textContent).toContain('next: check the retry path against staging');
    expect(strip.textContent).toMatch(/\+1890\/−122 since\s+you\s+reviewed/);
    expect(strip.textContent).toContain('Added the retry wrapper');
    await fireEvent.keyDown(strip, { key: 'v' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
    await fireEvent.keyDown(strip, { key: 'x' });
    await waitFor(() => expect(screen.queryByTestId('return-brief')).toBeNull());
  });

  it('clears claude_replied while the tab is shown (#489)', async () => {
    const w = { ...item(3), claude_replied: true };
    work.upsert(w);
    mountHeader(w);
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_seen')).toBe(true));
    await waitFor(() => expect(work.get(w.id)?.claude_replied).toBe(false));
  });

  it('not before the threshold, and never while Claude is working', async () => {
    const recent = { ...item(3), left_at: ago(5) };
    work.upsert(recent);
    const { unmount } = mountHeader(recent);
    await Promise.resolve();
    expect(screen.queryByTestId('return-brief')).toBeNull();
    unmount();
    // Leaving the item stamps when Louis left.
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_left')).toBe(true));

    const busy = { ...item(0), left_at: ago(120), next_note: 'look at the cache' }; // Claude is working
    work.upsert(busy);
    mountHeader(busy);
    await Promise.resolve();
    expect(screen.queryByTestId('return-brief')).toBeNull();
  });
});
