import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, sessions, settings, work } from '$lib/stores';

import { claudeOf } from '../live';
import Claude from './Claude.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  work.byId = {};
  await Promise.all([work.load(), sessions.load(), projects.load(), settings.load()]);
});

const item = (i: number): WorkItem => mock.state.work[i] as WorkItem;
const mount = (w: WorkItem) => render(Claude, { props: { item: w, open: true } });

describe('Story Claude section', () => {
  it('SHOP-142: 5 tasks, one in progress, "3 of 5" in the summary, Go to Claude', async () => {
    const w = item(0);
    mount(w);
    const list = await screen.findByTestId('claude-tasks');
    const rows = [...list.querySelectorAll('li')];
    expect(rows).toHaveLength(5);
    expect(rows.filter((r) => r.classList.contains('in_progress'))).toHaveLength(1);
    expect(screen.getByTestId('story-claude').querySelector('summary')!.textContent).toContain(
      '3 of 5 tasks',
    );
    expect(screen.getByRole('button', { name: 'Go to Claude' })).toBeTruthy();
    // Claude is working: Park is not allowed.
    expect(screen.queryByRole('button', { name: 'Park' })).toBeNull();
  });

  it('without a task list the section shows the last message only; e expands it', async () => {
    const w = item(0);
    const s = claudeOf(w)!;
    sessions.upsert({
      ...s,
      status: 'done',
      claude: { ...s.claude!, tasks: [], preview: 'Done: added the flag.' },
    });
    mount(w);
    const msg = await screen.findByTestId('claude-message');
    expect(msg.textContent).toContain('Done: added the flag.');
    expect(screen.queryByTestId('claude-tasks')).toBeNull();
    expect(msg.classList.contains('clamped')).toBe(true);
    await fireEvent.keyDown(screen.getByRole('group', { name: 'Claude' }), { key: 'e' });
    await waitFor(() => expect(msg.classList.contains('clamped')).toBe(false));
    expect(screen.getByRole('button', { name: 'Park' })).toBeTruthy();
  });
});
