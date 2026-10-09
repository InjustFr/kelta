import { beforeEach, describe, expect, it } from 'vitest';

import { dispatch, hasAction } from '$lib/actions';
import { ACTIONS } from '$lib/gen/actions';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, projects, sessions, toasts, ui, work } from '$lib/stores';
import { terminalUi } from '$lib/terminal/ui.svelte';

import './actions';

// Handlers owned by L2 (BUILD_PLAN §2.4).
const OWNED = ACTIONS.map((a) => a.id).filter(
  (id) =>
    id.startsWith('palette.') ||
    id.startsWith('project.') ||
    id === 'inbox.open' ||
    id.startsWith('tab.') ||
    id === 'session.new' ||
    id.startsWith('pane.') ||
    id === 'attention.next' ||
    id.startsWith('terminal.') ||
    id === 'editor.send_selection',
);

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  layout.byProject = {};
  ui.closeOverlay();
  ui.sheets = [];
  ui.inboxActive = false;
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

  it('project.goto.N, project.next and inbox.open navigate', async () => {
    await dispatch('project.goto.2');
    expect(projects.activeId).toBe('billing');
    await dispatch('project.next');
    expect(projects.activeId).toBe('kelta-tools');
    await dispatch('project.prev');
    expect(projects.activeId).toBe('billing');
    await dispatch('inbox.open');
    expect(ui.inboxActive).toBe(true);
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
});
