import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import { sheetRegistry, type RegisteredSheetKey } from '$app/registry';
import type { PluginInfo, WorkItem } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, plugins, projects, reviews, sessions, tickets, toasts, ui, work } from '$lib/stores';
import { idle } from '$lib/stores/loadable';

import { focus } from '../../shell/focus.svelte';
import { chordFor } from '../../shell/labels';
import { story } from './story/panel.svelte';
import { workUi } from './ui.svelte';
import WorkContext from './WorkContext.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  tickets.details = {};
  tickets.transitions = {};
  toasts.clear();
  work.byId = {};
  work.git = {};
  layout.byProject = {};
  ui.sheets = [];
  Object.assign(workUi, { menu: null });
  // ProjectRail reads git status at startup; the header itself never asks.
  await Promise.all([work.load(), sessions.load(), projects.load(), work.refreshStatus()]);
});

/** Renders the sheet the last action opened (the shell's SheetHost does this in the app). */
async function mountSheet(key: RegisteredSheetKey) {
  await waitFor(() => expect(ui.sheet?.key).toBe(key));
  const Sheet = (await sheetRegistry[key]()).default;
  return render(Sheet, { props: { ...ui.sheet!.props, onclose: () => ui.closeSheet(key) } });
}

function mountHeader(item: WorkItem) {
  return render(WorkContext, { props: { projectId: item.project_id, workItemId: item.id } });
}

const item = (i: number): WorkItem => mock.state.work[i] as WorkItem;

async function openMenu(): Promise<HTMLElement> {
  await fireEvent.click(await screen.findByTestId('work-menu-button'));
  return screen.findByRole('menu', { name: 'Work' });
}

describe('work tab header', () => {
  it('key, project and title, the stage track with whose move, and why in one sentence', async () => {
    const w = item(0); // SHOP-142, Claude working
    mountHeader(w);
    const header = await screen.findByTestId('work-header');
    expect(screen.getByTestId('work-header-key').textContent).toBe('SHOP-142');
    expect(header.textContent).toContain('Shop');
    await waitFor(() => expect(header.textContent).toContain('Rate-limit login attempts'));
    expect(header.dataset.phase).toBe('working');
    expect(header.querySelector('.track')?.getAttribute('data-mark')).toBe('wait');
    // All six stages named, the current one bold (#438).
    expect([...header.querySelectorAll('.track .name')].map((n) => n.textContent)).toEqual([
      'Ticket',
      'Claude',
      'PR',
      'Review',
      'Merge',
      'Deploy',
    ]);
    expect(header.querySelector('.track .name.now')?.textContent).toBe('Claude');
    expect(screen.getByTestId('work-why').textContent).toBe('Waiting on others: Claude working');
    // Status, PR, git and cost moved to the Story panel (#428).
    for (const id of ['work-status', 'work-phase', 'work-pr', 'ahead', 'work-branch'])
      expect(screen.queryByTestId(id)).toBeNull();
  });

  it('the primary shows the work.start chord; Leave focus shows the view.focus chord, not Esc Esc (#445)', async () => {
    mountHeader(item(3));
    const primary = await screen.findByTestId('work-primary');
    expect(chordFor('work.start')).toBeTruthy();
    expect(primary.querySelector('kbd')?.getAttribute('aria-label')).toBe(chordFor('work.start'));
    expect(screen.queryByTestId('work-leave-focus')).toBeNull();

    focus.on = true;
    try {
      const leave = await screen.findByTestId('work-leave-focus');
      const keys = [...leave.querySelectorAll('kbd')].map((k) => k.getAttribute('aria-label'));
      expect(keys).toEqual([chordFor('view.focus')]);
      expect(leave.title).toContain('or Esc Esc outside a terminal');
    } finally {
      focus.on = false;
    }
  });

  it('the mark follows the phase: to review is my move, a failed start is broken', async () => {
    const marks: [string, string][] = [];
    for (const i of [3, 1]) {
      const { unmount } = mountHeader(item(i));
      const header = await screen.findByTestId('work-header');
      marks.push([header.dataset.phase!, header.dataset.mark!]);
      unmount();
    }
    expect(marks).toEqual([
      ['to_review', 'you'],
      ['failed', 'broken'],
    ]);
  });

  it('re-reads git status when Claude stops (from the store)', async () => {
    const w = item(0);
    mountHeader(w);
    await screen.findByTestId('work-header');
    const before = mock.calls.filter((c) => c.cmd === 'work_status_all').length;
    work.upsert({ ...w, review_due: true });
    await waitFor(() =>
      expect(mock.calls.filter((c) => c.cmd === 'work_status_all').length).toBeGreaterThan(before),
    );
  });

  it('To review: the primary opens the delta zoomed in the work tab, R marks reviewed', async () => {
    const w = item(3); // SHOP-155, review_due
    mountHeader(w);
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('to_review'));
    await fireEvent.click(screen.getByRole('button', { name: 'Review changes' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_diff')).toBe(true));
    // No work tab for SHOP-155 in the fixtures: work_resume recreates it, the diff opens beside it.
    await waitFor(() =>
      expect(
        findContent(
          layout.get('shop')!,
          (c) => c.kind === 'terminal' && sessions.get(c.session_id)?.name === 'diff',
        ),
      ).toBeTruthy(),
    );
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'R' });
    await waitFor(() => expect(work.get(w.id)?.review_due).toBe(false));
  });

  it('the sentence opens the Story panel of its tab, and no pane', async () => {
    const w = item(0);
    await layout.ensure('shop');
    story.setOpen('shop-t1', false);
    mountHeader(w);
    await fireEvent.click(await screen.findByTestId('work-why'));
    expect(story.isOpen('shop-t1')).toBe(true);
    expect(findContent(layout.get('shop')!, (c) => c.kind === 'work_item')).toBeNull();
  });

  it('⌘. o opens the PR', async () => {
    const w = item(2); // gh-12, PR #13
    await reviews.load({ kind: 'all' }, 'authored', false);
    mountHeader(w);
    await fireEvent.keyDown(await openMenu(), { key: 'o' });
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'open_external')).toBe(true));
  });

  it('work menu: what can run now by object, then Not available now; hidden letters say why (#405)', async () => {
    mountHeader(item(0)); // SHOP-142, Claude working: no next step
    const menu = await openMenu();
    const entries = within(menu).getAllByRole('menuitem');
    const label = (e: HTMLElement) => e.querySelector('.label')?.textContent ?? '';
    expect(entries.map(label)).toEqual([
      'Review changes since last look',
      'Review the whole diff in nvim',
      'Add next: note…',
      'Go to Claude',
      'Finish…',
      'Open ticket',
      'Start another PR',
      'Next step',
      'Move to step…',
      'Ship',
      'Rebase onto main',
      'Mark reviewed',
    ]);
    // Headed by object (#439); only what could run soon is under "Not available now", its reason below.
    within(menu).getByText(`Ticket ${item(0).ticket!.key}`);
    within(menu).getByText(`Branch ${item(0).branch}`);
    const group = within(menu).getByText('Not available now (1)');
    const off = entries.filter((e) => e.getAttribute('aria-disabled') === 'true');
    expect(off).toHaveLength(1);
    expect(group.compareDocumentPosition(off[0]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(off[0]!.querySelector('.reason')?.textContent).toBe('Nothing to review');
    // A letter not listed still answers with its reason, and the menu stays.
    await fireEvent.keyDown(menu, { key: 'c' });
    expect(toasts.list.at(-1)?.toast.text).toBe('Continue rebase: Only while a rebase is stopped');
    await fireEvent.keyDown(menu, { key: 'C' });
    expect(toasts.list.at(-1)?.toast.text).toBe('Start Claude: Claude is already running');
    await fireEvent.keyDown(menu, { key: 'M' });
    expect(toasts.list.at(-1)?.toast.text).toBe('Merge when ready…: Needs an open PR');
    expect(screen.getByRole('menu', { name: 'Work' })).toBeTruthy();
  });

  it("the mock's SHOP-155 and SHOP-120 menus are headed by ticket, branch and PR (#439)", async () => {
    await reviews.load({ kind: 'all' }, 'authored', false);
    const byKey = (key: string) => mock.state.work.find((w) => w.ticket?.key === key) as WorkItem;
    const heads = async (w: WorkItem) => {
      const { unmount } = mountHeader(w);
      const menu = await openMenu();
      const out = [...menu.querySelectorAll('.group')].map((e) => e.textContent);
      workUi.menu = null;
      unmount();
      return out;
    };
    const shop155 = byKey('SHOP-155');
    expect(await heads(shop155)).toEqual(
      expect.arrayContaining(['Ticket SHOP-155', `Branch ${shop155.branch}`]),
    );
    const shop120 = byKey('SHOP-120');
    expect(await heads(shop120)).toEqual(
      expect.arrayContaining(['Ticket SHOP-120', `Branch ${shop120.branch}`, 'Pull request #305']),
    );
  });

  it('plugin work actions come after the built-ins, without letters, and run with the work item', async () => {
    const base = mock.state.plugins[0] as PluginInfo;
    const deploy = {
      id: 'deploy',
      title: 'Deploy to QA',
      do: [{ action: 'toast' as const, text: '{branch}', level: null }],
    };
    plugins.plugins = {
      ...idle(),
      data: [
        { ...base, id: 'qa', name: 'QA', contributes: { ...base.contributes, work_actions: [deploy] } },
        {
          ...base,
          id: 'off',
          name: 'Off',
          enabled: false,
          contributes: { ...base.contributes, work_actions: [deploy] },
        },
      ],
      fetchedAt: Date.now(),
    };
    const w = item(0);
    mountHeader(w);
    const menu = await openMenu();
    const entries = within(menu).getAllByRole('menuitem');
    expect(entries.length).toBe(13); // the 11 that can run, the enabled plugin's action, then 1 not available
    const last = entries[11]!;
    expect(last.textContent).toContain('Deploy to QA');
    expect(last.textContent).toContain('QA');
    await fireEvent.click(last);
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'command_run')).toBe(true));
    expect(mock.calls.find((c) => c.cmd === 'command_run')?.args).toMatchObject({
      command_id: 'qa/deploy',
      ctx: { work_item_id: w.id, ticket: w.ticket, repo_id: w.repo_id, extra: { project_id: w.project_id } },
    });
    plugins.plugins = { ...idle(), data: [], fetchedAt: Date.now() };
  });

  it('review checkouts are read-only: p r f l ⇧T ⇧M are not listed and say why', async () => {
    const r: WorkItem = {
      ...item(0),
      id: 'review-item',
      kind: 'review',
      ticket: null,
      review: { account: 'github-acme', repo: 'acme/shop-api', number: 311 },
    };
    work.upsert(r);
    mountHeader(r);
    const menu = await openMenu();
    const labels = within(menu)
      .getAllByRole('menuitem')
      .map((e) => e.querySelector('.label')?.textContent?.split(' ')[0]);
    for (const l of ['Ship', 'Push', 'Fix', 'Rebase', 'Link', 'Create', 'Merge'])
      expect(labels).not.toContain(l);
    await fireEvent.keyDown(menu, { key: 'r' });
    expect(toasts.list.at(-1)?.toast.text).toBe('Rebase onto main: Review checkout: read-only');
  });

  it('p ships through the Ship dialog', async () => {
    const w = { ...item(0) };
    mock.state.sessions.find((s) => s.id === w.session_ids[0])!.status = 'done';
    await sessions.load();
    mountHeader(w);
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'p' });
    await mountSheet('ship');
    const dialog = await screen.findByRole('dialog');
    const title = within(dialog).getByLabelText('Title') as HTMLInputElement;
    await waitFor(() => expect(title.value).toBe('SHOP-142: Rate-limit login attempts'));
    await fireEvent.input(title, { target: { value: 'My PR' } });
    // The fixture worktree is dirty: Ship anyway.
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Ship anyway' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_create_pr')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_create_pr').at(-1)?.args).toMatchObject({
      id: w.id,
      draft: { title: 'My PR', draft: false },
    });
    // PR open with unpushed commits: the next action is Push (another lane's).
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('unpushed'));
  });

  it('Shift+F finishes: lists the dirty files and only removes with Force', async () => {
    const w = item(1);
    mountHeader(w);
    const menu = await openMenu();
    await fireEvent.keyDown(menu, { key: 'F' });
    await mountSheet('finish');
    const dialog = await screen.findByRole('dialog');
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Finish' }));
    const dirty = await screen.findByTestId('finish-dirty');
    expect(dirty.textContent).toContain('src/invoice.rs');
    await fireEvent.click(screen.getByRole('button', { name: 'Force remove' }));
    await waitFor(() => expect(mock.calls.filter((c) => c.cmd === 'work_finish')).toHaveLength(2));
    expect(mock.calls.filter((c) => c.cmd === 'work_finish').at(-1)?.args).toMatchObject({
      opts: { force: true },
    });
    await waitFor(() => expect(work.get(w.id)?.state.kind).toBe('finished'));
  });

  it('a failed start: the primary retries the step', async () => {
    const w = item(1);
    expect(w.state.kind).toBe('failed');
    mountHeader(w);
    await fireEvent.click(await screen.findByRole('button', { name: /^Retry / }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_retry_step')).toBe(true));
  });

  it('rebase: stopped on conflicts → Continue → Rebased → Force push… (confirmed, lease)', async () => {
    const w = item(2); // PR open, Claude idle
    mountHeader(w);
    await fireEvent.click(within(await openMenu()).getByRole('menuitem', { name: /Rebase onto main/ }));
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('rebase_stopped'));
    expect(screen.getByRole('button', { name: 'Ask Claude to resolve' })).toBeTruthy();
    await fireEvent.click(within(await openMenu()).getByRole('menuitem', { name: /Continue rebase/ }));
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('rebased'));
    await fireEvent.click(screen.getByRole('button', { name: 'Force push…' }));
    // The dialog is a sheet entry: it is mounted by the sheet host, here we assert what it asks.
    const { ui } = await import('$lib/stores');
    expect(ui.sheet?.key).toBe('work_dialog');
    expect(ui.sheet?.props).toMatchObject({ tone: 'danger', title: 'Force push' });
    expect(String(ui.sheet?.props.text)).toMatch(
      /Rewrites feat\/gh-12-json-output on origin \(#13\)\. The lease checks origin is still at \w{7}\./,
    );
    expect(mock.calls.some((c) => c.cmd === 'work_push')).toBe(false);
    ui.closeSheet();
  });

  it('remote commits get "Rebase onto origin/<branch>", never Force push', async () => {
    const w = item(2); // a reviewer's suggestion commit landed on the remote branch
    mock.state.remoteNew[w.id] = 2;
    await work.refreshStatus();
    const claude = mock.state.sessions.find((x) => w.session_ids.includes(x.id) && x.kind.type === 'claude')!;
    claude.status = 'done'; // it was asking for permission: rebase would be refused
    mountHeader(w);
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).toBe('remote_new'));
    expect(screen.queryByRole('button', { name: 'Force push…' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: `Rebase onto origin/${w.branch}` }));
    // The rebase onto the remote branch is followed by the normal rebase onto base (FLOW §4.4).
    await waitFor(() =>
      expect(
        mock.calls
          .filter((c) => c.cmd === 'work_rebase')
          .map((c) => (c.args as { op: { onto: string } }).op.onto),
      ).toEqual(['remote_branch', 'base']),
    );
    await waitFor(() => expect(screen.getByTestId('work-header').dataset.phase).not.toBe('remote_new'));
  });

  it('shows a placeholder for an unknown work item', async () => {
    render(WorkContext, { props: { projectId: 'shop', workItemId: 'nope' } });
    expect(await screen.findByText('Work item not found')).toBeTruthy();
  });
});
