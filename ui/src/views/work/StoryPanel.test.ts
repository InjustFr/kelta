import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects, sessions, work } from '$lib/stores';

import { story } from './story/panel.svelte';
import StoryPanel from './StoryPanel.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  work.byId = {};
  layout.byProject = {};
  await Promise.all([work.load(), sessions.load(), projects.load()]);
});

const item = (i: number): WorkItem => mock.state.work[i] as WorkItem;
const mount = (w: WorkItem, tabId = 'tab-x') =>
  render(StoryPanel, { props: { projectId: w.project_id, tabId, workItemId: w.id } });

describe('Story panel', () => {
  it('sections in stage order; the current stage (Claude for SHOP-142) is open, Log folded', async () => {
    mount(item(0));
    const panel = await screen.findByTestId('story-panel');
    const ids = [...panel.querySelectorAll('details[data-testid^="story-"]')].map((e) =>
      e.getAttribute('data-testid'),
    );
    expect(ids).toEqual(['story-ticket', 'story-claude', 'story-changes', 'story-pr', 'story-log']);
    expect(screen.getByTestId('story-claude').hasAttribute('open')).toBe(true);
    expect(screen.getByTestId('story-ticket').hasAttribute('open')).toBe(false);
    expect(screen.getByTestId('story-log').hasAttribute('open')).toBe(false);
    expect(screen.getByTestId('story-pr').textContent).toContain('No pull request');
  });

  it('Log opens by itself on a failed step; Retry and Skip step rerun the saga', async () => {
    mount(item(1)); // 4567: tracker_side_effects failed
    const log = await screen.findByTestId('story-log');
    expect(log.hasAttribute('open')).toBe(true);
    expect(log.textContent).toMatch(/failed/);
    await fireEvent.click(screen.getByRole('button', { name: 'Skip step' }));
    await waitFor(() =>
      expect(mock.calls.find((c) => c.cmd === 'work_retry_step')?.args).toMatchObject({
        step: 'skip:tracker_side_effects',
      }),
    );
    await waitFor(() => expect(screen.getByTestId('story-log').hasAttribute('open')).toBe(false));
  });

  it('the delta reads +a −b, n files', async () => {
    mount(item(3)); // SHOP-155
    expect((await screen.findByTestId('story-changes')).textContent).toContain('+1890 −122, 9 files');
  });

  it('a closed tab keeps the panel mounted but hidden; toggling persists per tab', async () => {
    story.setOpen('tab-x', false);
    mount(item(0));
    const panel = await screen.findByTestId('story-panel');
    expect(panel.classList.contains('closed')).toBe(true);
    story.toggle('tab-x');
    await waitFor(() => expect(panel.classList.contains('closed')).toBe(false));
    expect(story.isOpen('tab-y')).toBe(story.wide);
    expect(JSON.parse(localStorage.getItem('kelta.story.v1')!).open['tab-x']).toBe(true);
  });
});
