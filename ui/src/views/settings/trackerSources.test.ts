// Settings → Sources (each source's Board block, label, projects, routes), Settings → Projects'
// "Fed by", and the hand-offs of settings.open.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { dispatch } from '$lib/actions';
import type { JsonValue, SourceRoute, TrackerBinding, TrackerView } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { createMockTransport, lastSettingsValue, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, settings, ui } from '$lib/stores';

import { confirms } from '../../shell/confirm.svelte';
import { takeTomlRequest } from './actions';
import { iterationWord } from './lib/sources.svelte';
import Projects from './sections/Projects.svelte';
import Sources from './sections/Sources.svelte';
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

const views = () => screen.getAllByTestId('tracker-source');
type SourceList = {
  id: string;
  label: string;
  projects: string[];
  routes: SourceRoute[];
  tracker: TrackerBinding;
}[];
/** The last `sources` list written to the global settings. */
const lastSources = () => lastSettingsValue<SourceList>(mock);
const shop = () => lastSources()?.find((s) => s.id === 'shop');

describe('Board block of a source', () => {
  it("lists the source's views with account, who and the current sprint toggle", async () => {
    render(TrackerSources, { sourceId: 'shop' });
    const ids = settings
      .value()!
      .sources.find((s) => s.id === 'shop')!
      .tracker.views.map((v) => v.id);
    expect(views().map((r) => r.dataset.viewId)).toEqual(ids);
    const first = within(views()[0]!);
    expect(first.getByText('jira-acme')).toBeTruthy();
    expect(first.getByRole('switch', { name: 'Current sprint only' })).toBeTruthy();
  });

  it('saves who, current iteration and removal to the global sources', async () => {
    render(TrackerSources, { sourceId: 'shop' });
    const id = views()[0]!.dataset.viewId!;
    await fireEvent.change(within(views()[0]!).getByRole('combobox', { name: /^Whose tickets/ }), {
      target: { value: 'unassigned' },
    });
    await waitFor(() => expect(shop()?.tracker.views.find((v) => v.id === id)?.who).toBe('unassigned'));
    expect(mock.calls.filter((c) => c.cmd === 'settings_set').at(-1)?.args).toMatchObject({
      layer: 'global',
      path: 'sources',
    });
    // a view with a query can go back to letting the query decide
    await fireEvent.change(within(views()[0]!).getByRole('combobox', { name: /^Whose tickets/ }), {
      target: { value: '' },
    });
    await waitFor(() => expect(shop()?.tracker.views.find((v) => v.id === id)?.who).toBeNull());

    await fireEvent.click(within(views()[0]!).getByRole('switch', { name: 'Current sprint only' }));
    await waitFor(() => expect(shop()?.tracker.views.find((v) => v.id === id)?.current_iteration).toBe(true));

    const n = views().length;
    await fireEvent.click(within(views()[0]!).getByRole('button', { name: /^Remove / }));
    await waitFor(() => expect(confirms.current?.title).toMatch(/^Remove /));
    confirms.answer('remove');
    await waitFor(() => expect(shop()?.tracker.views.map((v) => v.id)).not.toContain(id));
    await waitFor(() => expect(views()).toHaveLength(n - 1));
    expect(mock.calls.some((c) => c.cmd === 'project_update')).toBe(false);
  });

  it("binds a Jira view to a board and to statuses of the source's open columns", async () => {
    const plain = JSON.parse(JSON.stringify(settings.value()!.sources)) as SourceList;
    const sources = plain.map((s) =>
      s.id === 'shop'
        ? {
            ...s,
            tracker: {
              ...s.tracker,
              columns: [
                {
                  id: 'doing',
                  label: 'Doing',
                  categories: ['in_progress'],
                  names: ['In Progress', 'Code Review'],
                },
                { id: 'done', label: 'Done', categories: ['done'], names: ['Resolved'] },
              ],
            },
          }
        : s,
    );
    await ipc.settingsSet({ layer: 'global', path: 'sources', value: sources as unknown as JsonValue });
    await settings.load();
    render(TrackerSources, { sourceId: 'shop' });
    const id = views()[0]!.dataset.viewId!;
    const row = within(views()[0]!);
    const board = row.getByRole('combobox', { name: /^Board of / });
    await waitFor(() => expect(within(board).getByRole('option', { name: 'SHOP board' })).toBeTruthy());
    await fireEvent.change(board, { target: { value: '12' } });
    await waitFor(() => expect(shop()?.tracker.views.find((v) => v.id === id)?.board_id).toBe(12));

    // Done statuses are not offered; ticking then unticking the only one clears the filter.
    await waitFor(() => expect(row.getByRole('switch', { name: 'Code Review' })).toBeTruthy());
    expect(row.queryByRole('switch', { name: 'Resolved' })).toBeNull();
    await fireEvent.click(row.getByRole('switch', { name: 'Code Review' }));
    await waitFor(() =>
      expect(shop()?.tracker.views.find((v) => v.id === id)?.statuses).toEqual(['Code Review']),
    );
    await fireEvent.click(row.getByRole('switch', { name: 'Code Review' }));
    await waitFor(() => expect(shop()?.tracker.views.find((v) => v.id === id)?.statuses).toBeNull());

    await fireEvent.change(board, { target: { value: '' } });
    await waitFor(() => expect(shop()?.tracker.views.find((v) => v.id === id)?.board_id).toBeNull());
  });

  it('Add view… opens the source picker on that source', async () => {
    render(TrackerSources, { sourceId: 'billing' });
    await fireEvent.click(screen.getByTestId('add-source'));
    expect(ui.sheets.at(-1)).toEqual({ key: 'tracker.source_picker', props: { sourceId: 'billing' } });
  });
});

describe('Settings → Sources', () => {
  const mountSources = () => render(Sources, { layer: 'global', projectId: null, repoId: null });
  const card = (id: string) =>
    within(document.querySelector<HTMLElement>(`[data-source-card="${id}"]`) as HTMLElement);

  it('edits label, projects and routes of a source, and removes it after a confirmation', async () => {
    mountSources();
    await waitFor(() => expect(screen.getAllByTestId('source-card')).toHaveLength(4));
    const label = card('shop').getByLabelText('Label');
    await fireEvent.input(label, { target: { value: 'Product board' } });
    await fireEvent.change(label);
    await waitFor(() => expect(shop()?.label).toBe('Product board'));

    await fireEvent.click(card('shop').getByRole('button', { name: 'Billing' }));
    await waitFor(() => expect(shop()?.projects).toEqual(['shop', 'billing']));

    await fireEvent.click(card('shop').getByRole('button', { name: 'Add route' }));
    await waitFor(() => expect(shop()?.routes).toHaveLength(1));
    expect(shop()!.routes[0]).toMatchObject({ project: 'shop', match: { component: '' } });
    await fireEvent.change(card('shop').getByLabelText('Match of route 1'), { target: { value: 'label' } });
    await waitFor(() => expect(shop()!.routes[0]!.match).toEqual({ component: null, label: '', type: null }));
    const value = card('shop').getByLabelText('Value of route 1');
    await fireEvent.input(value, { target: { value: 'web' } });
    await fireEvent.change(value);
    await waitFor(() => expect(shop()!.routes[0]!.match.label).toBe('web'));
    await fireEvent.change(card('shop').getByLabelText('Project of route 1'), {
      target: { value: 'billing' },
    });
    await waitFor(() => expect(shop()!.routes[0]).toMatchObject({ project: 'billing' }));
    await fireEvent.click(card('shop').getByRole('button', { name: 'Add route' }));
    await waitFor(() => expect(shop()?.routes).toHaveLength(2));
    await fireEvent.click(card('shop').getByRole('button', { name: 'Move route 2 up' }));
    await waitFor(() => expect(shop()!.routes[1]!.match.label).toBe('web'));
    // unticking a project drops its routes
    await fireEvent.click(card('shop').getByRole('button', { name: 'Billing' }));
    await waitFor(() => expect(shop()?.routes.map((r) => r.project)).toEqual(['shop']));

    // columns and status map render from the schema
    const columns = await card('shop').findByLabelText('Columns of Product board');
    const todo = [{ id: 'todo', label: 'To do', categories: ['todo'], names: [] }];
    await fireEvent.input(columns, { target: { value: JSON.stringify(todo) } });
    await fireEvent.change(columns);
    await waitFor(() => expect(shop()?.tracker.columns).toEqual(todo));
    const done = card('shop').getByText('Done', { selector: 'span' }).parentElement!;
    await fireEvent.change(within(done).getAllByRole('combobox')[1]!, { target: { value: 'in_review' } });
    await waitFor(() => expect(shop()?.tracker.status_map.done).toEqual({ category: 'in_review' }));

    await fireEvent.click(card('ops').getByRole('button', { name: 'Remove Ops board' }));
    await waitFor(() => expect(confirms.current?.title).toBe('Remove Ops board?'));
    confirms.answer('remove');
    await waitFor(() => expect(lastSources()?.map((s) => s.id)).toEqual(['shop', 'billing', 'kelta-tools']));
  });

  it('brings the source asked by settings.open into view, and adds a source with the picker', async () => {
    const seen = vi.fn();
    Element.prototype.scrollIntoView = seen;
    await import('./actions');
    await dispatch('settings.open', { section: 'sources', source_id: 'billing' });
    mountSources();
    await waitFor(() => expect(seen).toHaveBeenCalled());
    expect((seen.mock.contexts[0] as HTMLElement).dataset.sourceCard).toBe('billing');
    // a save replaces the sources list: it must not scroll back to the card
    await new Promise((r) => setTimeout(r)); // the settings reload lands first: it would reset the typed label
    const label = card('shop').getByLabelText('Label');
    await fireEvent.input(label, { target: { value: 'Renamed' } });
    await fireEvent.change(label);
    await waitFor(() => expect(shop()?.label).toBe('Renamed'));
    await tick();
    expect(seen).toHaveBeenCalledTimes(1);
    await fireEvent.click(screen.getByRole('button', { name: 'Add source' }));
    expect(ui.sheets.at(-1)).toEqual({ key: 'tracker.source_picker', props: {} });
  });

  it('Settings → Projects shows which sources feed a project, and no Tracker block', async () => {
    render(Projects, { layer: 'global', projectId: null, repoId: null });
    const row = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('[data-project-id="shop"]');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(within(row).getByTestId('fed-by').textContent).toBe('Fed by Shop board');
    expect(screen.queryByTestId('tracker-block')).toBeNull();
  });

  it('a source that omits routes, projects or views still renders and edits (#281)', async () => {
    // the TOML keeps only what the user wrote; core (and the mock) fill serde defaults
    const sparse: JsonValue[] = [
      {
        id: 'mine',
        label: 'Mine',
        tracker: { account: 'jira-acme', views: [{ id: 'v', label: 'V', jql: 'x' }] },
      },
      { id: 'fed', label: 'Fed', projects: ['shop'], tracker: { account: 'jira-acme' } },
    ];
    await ipc.settingsSet({ layer: 'global', path: 'sources', value: sparse });
    await settings.load();
    expect(settings.hasSources()).toBe(true);
    render(Projects, { layer: 'global', projectId: null, repoId: null });
    const row = await waitFor(() => document.querySelector<HTMLElement>('[data-project-id="shop"]')!);
    expect(within(row).getByTestId('fed-by').textContent).toBe('Fed by Fed');
    mountSources();
    await waitFor(() => expect(screen.getAllByTestId('source-card')).toHaveLength(2));
    await fireEvent.click(card('fed').getByRole('button', { name: 'Add route' }));
    await waitFor(() => expect(lastSources()?.find((s) => s.id === 'fed')?.routes).toHaveLength(1));
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
