import { beforeEach, describe, expect, it, vi } from 'vitest';

import { dispatch, hasAction } from '$lib/actions';
import { ACTIONS } from '$lib/gen/actions';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects, sessions, toasts, ui, work } from '$lib/stores';
import { terminalUi } from '$lib/terminal/ui.svelte';

import { openFileLink } from './actions';
import { confirms } from './confirm.svelte';

// Handlers owned by L2 (BUILD_PLAN §2.4).
const OWNED = ACTIONS.map((a) => a.id).filter(
  (id) =>
    id.startsWith('palette.') ||
    id.startsWith('project.') ||
    id === 'inbox.open' ||
    id.startsWith('tab.') ||
    id === 'session.new' ||
    id.startsWith('pane.') ||
    id.startsWith('attention.') ||
    id.startsWith('nav.') ||
    id.startsWith('terminal.') ||
    id === 'editor.send_selection' ||
    id === 'editor.quickfix_claude' ||
    id === 'app.restart',
);

const CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const NVIM = '0199a6b2-0000-7000-8000-000000000002';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  layout.byProject = {};
  ui.closeOverlay();
  ui.sheets = [];
  ui.view = null;
  terminalUi.searchSession = null;
  toasts.clear();
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  await layout.ensure('shop');
});

describe('shell actions', () => {
  it('registers a handler for every action id owned by L2', () => {
    expect(OWNED.length).toBeGreaterThan(25);
    for (const id of OWNED) expect(hasAction(id), id).toBe(true);
  });

  it('does not register handlers owned by other lanes', () => {
    for (const id of ['tickets.open', 'reviews.open', 'work.start', 'settings.open', 'window.toggle']) {
      expect(hasAction(id), id).toBe(false);
    }
  });

  it('palette.open and project.switcher toggle their overlay', async () => {
    await dispatch('palette.open');
    expect(ui.overlay).toBe('palette');
    await dispatch('palette.open');
    expect(ui.overlay).toBeNull();
    await dispatch('project.switcher');
    expect(ui.overlay).toBe('switcher');
  });

  it('session.new opens the new-session sheet', async () => {
    await dispatch('session.new');
    expect(ui.sheet?.key).toBe('session_new');
  });

  it('project.goto.N (rail row N), project.next and inbox.open navigate', async () => {
    await layout.ensure('billing');
    await dispatch('project.goto.2'); // work rows: one per open work tab
    expect(projects.activeId).toBe('billing');
    await dispatch('project.next');
    expect(projects.activeId).toBe('kelta-tools');
    await dispatch('project.prev');
    expect(projects.activeId).toBe('billing');
    await dispatch('inbox.open');
    expect(ui.view).toBe('now');
    await dispatch('inbox.open'); // the palette and the HUD only open it (#415)
    expect(ui.view).toBe('now');
  });

  it('terminal.search targets the focused session', async () => {
    await dispatch('terminal.search');
    expect(terminalUi.searchSession).toBe('0199a6b2-0000-7000-8000-000000000001');
  });

  it('terminal.copy and terminal.paste without a live view do nothing', async () => {
    await expect(dispatch('terminal.copy')).resolves.toBe(true);
    await expect(dispatch('terminal.paste')).resolves.toBe(true);
  });

  it('editor.send_selection needs an editor pane and a Claude session in the tab', async () => {
    await dispatch('editor.send_selection'); // focused pane is the Claude session
    expect(toasts.list.at(-1)?.toast.text).toContain('editor pane');
    await dispatch('pane.focus_right'); // nvim
    await dispatch('editor.send_selection');
    const call = mock.calls.find((c) => c.cmd === 'editor_send_selection');
    expect(call?.args).toEqual({
      editor_session: '0199a6b2-0000-7000-8000-000000000002',
      claude_session: '0199a6b2-0000-7000-8000-000000000001',
    });
  });

  it("file links open in the tab's nvim, or the work item's nvim when the session has one", async () => {
    await openFileLink(CLAUDE, '/w/src/a.rs', 42, false);
    expect(mock.calls.find((c) => c.cmd === 'editor_open')?.args).toEqual({
      target: { kind: 'session', id: NVIM },
      path: '/w/src/a.rs',
      line: 42,
    });
    // Another work item's nvim (same project) and a nvim in another project are never picked.
    const claude = sessions.get(CLAUDE)!;
    sessions.upsert({ ...claude, work_item_id: 'w-1' });
    sessions.upsert({ ...sessions.get(NVIM)!, id: 'other-item', work_item_id: 'w-2' });
    sessions.upsert({ ...sessions.get(NVIM)!, id: 'item-nvim', work_item_id: 'w-1' });
    await openFileLink(CLAUDE, '/w/b.rs', 3, false);
    expect(mock.calls.filter((c) => c.cmd === 'editor_open').at(-1)?.args).toMatchObject({
      target: { kind: 'session', id: 'item-nvim' },
      line: 3,
    });
  });

  it('editor.quickfix_claude lists exactly the files Claude touched', async () => {
    const claude = sessions.get(CLAUDE)!;
    sessions.upsert({ ...claude, claude: { ...claude.claude!, files_touched: [] } });
    await dispatch('editor.quickfix_claude');
    expect(toasts.list.at(-1)?.toast.text).toContain('not touched');
    const files = ['/w/src/a.rs', '/w/b.rs'];
    sessions.upsert({ ...claude, claude: { ...claude.claude!, files_touched: files } });
    await dispatch('editor.quickfix_claude');
    expect(mock.calls.find((c) => c.cmd === 'editor_quickfix')?.args).toEqual({
      target: { kind: 'session', id: NVIM },
      files,
    });
  });

  it('app.restart asks first only when a working Claude session would stop', async () => {
    const restarts = () => mock.calls.filter((c) => c.cmd === 'app_restart').length;
    await dispatch('app.restart');
    expect(confirms.current).toBeNull();
    expect(restarts()).toBe(1);

    mock.state.update.restart_needs_confirm = true;
    const cancelled = dispatch('app.restart');
    await vi.waitFor(() => expect(confirms.current?.body).toContain('will stop'));
    confirms.answer(null);
    await cancelled;
    expect(restarts()).toBe(1);

    const confirmed = dispatch('app.restart');
    await vi.waitFor(() => expect(confirms.current).not.toBeNull());
    confirms.answer('restart');
    await confirmed;
    expect(restarts()).toBe(2);
  });
});
