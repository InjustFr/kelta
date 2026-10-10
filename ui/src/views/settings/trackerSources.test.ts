// Settings → Projects → Tracker block, and the Edit TOML hand-off of settings.open.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { dispatch } from '$lib/actions';
import type { ProjectPatch, TrackerView } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, settings, ui } from '$lib/stores';

import { takeTomlRequest } from './actions';
import { iterationWord } from './lib/sources.svelte';
import TrackerSources from './TrackerSources.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  await ipc.settingsSet({ layer: 'global', path: 'accounts.jira-acme', value: { kind: 'jira' } });
  await Promise.all([projects.load(), settings.load()]);
  ui.sheets = [];
});

const sources = () => screen.getAllByTestId('tracker-source');
const lastPatch = () =>
  (mock.calls.filter((c) => c.cmd === 'project_update').at(-1)?.args as { patch: ProjectPatch } | undefined)
    ?.patch;

describe('Tracker block', () => {
  it("lists the project's sources with account, who and the current sprint toggle", async () => {
    render(TrackerSources, { projectId: 'shop' });
    const views = projects.byId('shop')!.tracker!.views;
    expect(sources().map((r) => r.dataset.viewId)).toEqual(views.map((v) => v.id));
    const first = within(sources()[0]!);
    expect(first.getByText('jira-acme')).toBeTruthy();
    expect(first.getByRole('switch', { name: 'Current sprint only' })).toBeTruthy();
  });

  it('saves who, current iteration and removal through project_update', async () => {
    render(TrackerSources, { projectId: 'shop' });
    const id = sources()[0]!.dataset.viewId!;
    await fireEvent.change(within(sources()[0]!).getByRole('combobox'), { target: { value: 'unassigned' } });
    await waitFor(() => expect(lastPatch()?.tracker?.views.find((v) => v.id === id)?.who).toBe('unassigned'));

    await fireEvent.click(within(sources()[0]!).getByRole('switch'));
    await waitFor(() =>
      expect(lastPatch()?.tracker?.views.find((v) => v.id === id)?.current_iteration).toBe(true),
    );

    const n = sources().length;
    await fireEvent.click(within(sources()[0]!).getByRole('button', { name: /^Remove / }));
    await waitFor(() => expect(lastPatch()?.tracker?.views.map((v) => v.id)).not.toContain(id));
    await waitFor(() => expect(sources()).toHaveLength(n - 1));
  });

  it('opens the source picker for the shown project', async () => {
    render(TrackerSources, { projectId: 'billing' });
    await fireEvent.click(screen.getByTestId('add-source'));
    expect(ui.sheets.at(-1)).toEqual({ key: 'tracker.source_picker', props: { projectId: 'billing' } });
  });

  it('says so when the project has no source', async () => {
    await ipc.projectUpdate({
      id: 'billing',
      patch: {
        name: null,
        color: null,
        icon: null,
        default_template: null,
        repos: null,
        tracker: null,
        remove_tracker: true,
      },
    });
    await projects.load();
    render(TrackerSources, { projectId: 'billing' });
    expect(screen.getByText('No ticket source for Billing.')).toBeTruthy();
  });

  it('names the iteration per provider and hides it for gitea', () => {
    expect(iterationWord('jira')).toBe('sprint');
    expect(iterationWord('linear')).toBe('cycle');
    expect(iterationWord('github')).toBe('iteration');
    expect(iterationWord('gitea')).toBeNull();
    expect(iterationWord(undefined)).toBeNull();
    const repoView = { project_v2: null } as TrackerView;
    expect(iterationWord('github', repoView)).toBeNull();
    expect(iterationWord('github', { project_v2: {} } as TrackerView)).toBe('iteration');
  });
});

describe('settings.open toml', () => {
  it('hands the project to the next Settings pane, once', async () => {
    await import('./actions');
    await dispatch('settings.open', { section: 'projects', toml: 'shop' });
    expect(takeTomlRequest()).toBe('shop');
    expect(takeTomlRequest()).toBeNull();
  });
});
