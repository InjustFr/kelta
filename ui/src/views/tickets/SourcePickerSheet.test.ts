// "Add a ticket source" against the mock transport (fixtures: mock/fixtures/sources.json).
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { registerAction } from '$lib/actions';
import type { ProjectPatch, TrackerView } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport, type IpcTransport } from '$lib/ipc/transport';
import { projects, settings } from '$lib/stores';

import SourcePickerSheet from './SourcePickerSheet.svelte';

let mock: MockControls;
let transport: IpcTransport;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  transport = created.transport;
  setTransport(created.transport);
  for (const [id, kind] of [
    ['jira-acme', 'jira'],
    ['github-oss', 'github'],
    ['redmine-corp', 'redmine'],
  ])
    await ipc.settingsSet({ layer: 'global', path: `accounts.${id}`, value: { kind } });
  await Promise.all([projects.load(), settings.load()]);
});

const mount = (projectId: string, onclose = vi.fn()) =>
  render(SourcePickerSheet, { props: { projectId, onclose } });
const rows = () => screen.queryAllByTestId('source-hit');
const labels = () => rows().map((r) => r.querySelector('.label')?.textContent);
const lastPatch = () =>
  (mock.calls.filter((c) => c.cmd === 'project_update').at(-1)?.args as { patch: ProjectPatch } | undefined)
    ?.patch;

describe('SourcePickerSheet', () => {
  it('lists the binding account sources and searches them, last request wins', async () => {
    mount('shop');
    await waitFor(() => expect(rows()).toHaveLength(4));
    await fireEvent.input(screen.getByTestId('source-search'), { target: { value: 'board' } });
    await waitFor(() => expect(labels()).toEqual(['SHOP board', 'SHOP board, current sprint']));
    const searches = mock.calls.filter((c) => c.cmd === 'tracker_sources').map((c) => c.args);
    expect(searches.at(-1)).toEqual({ account_id: 'jira-acme', query: 'board' });
  });

  it('drops an older search that answers after a newer one', async () => {
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    setTransport({
      ...transport,
      invoke: async (cmd, args, opts) => {
        const out = transport.invoke(cmd, args, opts);
        if (cmd === 'tracker_sources' && (args as { query: string }).query === 'b') await held;
        return out;
      },
    } as IpcTransport);
    mount('shop');
    await waitFor(() => expect(rows()).toHaveLength(4));
    const input = screen.getByTestId('source-search');
    await fireEvent.input(input, { target: { value: 'b' } });
    await waitFor(() =>
      expect(mock.calls.some((c) => (c.args as { query?: string })?.query === 'b')).toBe(true),
    );
    await fireEvent.input(input, { target: { value: 'board' } });
    await waitFor(() => expect(labels()).toEqual(['SHOP board', 'SHOP board, current sprint']));
    release();
    await new Promise((r) => setTimeout(r, 20));
    expect(labels()).toEqual(['SHOP board', 'SHOP board, current sprint']);
  });

  it('adds the picked hit with who mine, then shows it as Added', async () => {
    mount('shop');
    await waitFor(() => expect(labels()).toContain('Hot bugs'));
    const before = projects.byId('shop')!.tracker!.views.length;
    await fireEvent.click(rows().find((r) => r.textContent?.includes('Hot bugs'))!);
    await waitFor(() => expect(lastPatch()).toBeDefined());
    const views = lastPatch()!.tracker!.views;
    expect(views).toHaveLength(before + 1);
    expect(views.at(-1)).toMatchObject({
      id: 'src-filter-10',
      who: 'mine',
      account: null,
      jql: expect.any(String),
    });
    await waitFor(() =>
      expect(rows().find((r) => r.textContent?.includes('Hot bugs'))?.textContent).toContain('Added'),
    );
    // Enter on an added hit does nothing
    const calls = mock.calls.length;
    const hot = rows().findIndex((r) => r.textContent?.includes('Hot bugs'));
    const input = screen.getByTestId('source-search');
    for (let i = 0; i < hot; i++) await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(mock.calls.filter((c) => c.cmd === 'project_update')).toHaveLength(1);
    expect(mock.calls.length).toBe(calls);
  });

  it('keeps view.account when the hit comes from another account, and dedupes the id', async () => {
    mount('shop');
    await waitFor(() => expect(rows()).toHaveLength(4));
    await fireEvent.change(screen.getByLabelText('Account'), { target: { value: 'github-oss' } });
    await waitFor(() => expect(labels()).toEqual(['kelta/tools', 'kelta/site']));
    await fireEvent.keyDown(screen.getByTestId('source-search'), { key: 'Enter' });
    await waitFor(() => expect(lastPatch()).toBeDefined());
    const added = lastPatch()!.tracker!.views.at(-1) as TrackerView;
    expect(added).toMatchObject({ id: 'src-kelta-tools', account: 'github-oss', who: 'mine' });
    expect(lastPatch()!.tracker!.account).toBe('jira-acme');
  });

  it('creates the binding on the chosen account when the project has none', async () => {
    await ipc.projectUpdate({
      id: 'kelta-tools',
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
    mount('kelta-tools');
    await fireEvent.change(screen.getByLabelText('Account'), { target: { value: 'github-oss' } });
    await waitFor(() => expect(labels()).toContain('kelta/site'));
    await fireEvent.click(rows().find((r) => r.textContent?.includes('kelta/site'))!);
    await waitFor(() => expect(lastPatch()).toBeDefined());
    expect(lastPatch()!.tracker).toMatchObject({
      account: 'github-oss',
      views: [{ id: 'src-kelta-site', account: null, who: 'mine' }],
    });
  });

  it('falls back to Edit TOML when the provider cannot list sources', async () => {
    const open = vi.fn();
    const off = registerAction('settings.open', open);
    const onclose = vi.fn();
    mount('billing', onclose);
    await waitFor(() =>
      expect(screen.getByTestId('source-unsupported').textContent).toBe("Redmine can't list sources here."),
    );
    await fireEvent.click(screen.getByRole('button', { name: 'Edit TOML' }));
    expect(onclose).toHaveBeenCalled();
    expect(open).toHaveBeenCalledWith({ section: 'projects', toml: 'billing' });
    off();
  });
});
