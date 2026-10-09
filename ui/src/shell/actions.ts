// Action handlers owned by the shell (BUILD_PLAN §2.4): palette.*, project.*, inbox.open, tab.*,
// session.new, pane.*, attention.next, terminal.*, editor.send_selection. Loaded eagerly by main.ts.

import { registerAction } from '$lib/actions';
import { editorSendSelection, settingsSet } from '$lib/ipc/commands';
import { allPanes, paneSession } from '$lib/layout';
import { layout, projects, sessions, toasts, ui } from '$lib/stores';
import { terminalPool } from '$lib/terminal';
import { terminalUi } from '$lib/terminal/ui.svelte';
import type { TerminalView } from '$lib/terminal/view';

import {
  attentionNext,
  closeFocusedPane,
  currentTab,
  cycleProject,
  cycleTab,
  focusDirection,
  focusedSession,
  focusedSessionId,
  gotoProjectIndex,
  openInbox,
  splitFocused,
  toggleZoomFocused,
} from './nav';

function focusedView(): TerminalView | null {
  const id = focusedSessionId();
  return id ? terminalPool.get<TerminalView>(id) : null;
}

/** Sends the editor's visual selection to the Claude session of the same tab. */
async function sendSelection(): Promise<void> {
  const editor = focusedSession();
  const tab = currentTab();
  if (!editor || editor.kind.type !== 'editor' || !tab) {
    toasts.info('Focus an editor pane to send its selection to Claude');
    return;
  }
  const claude = allPanes(tab.root)
    .map((p) => paneSession(p))
    .map((id) => (id ? sessions.get(id) : null))
    .find((s) => s?.kind.type === 'claude');
  if (!claude) {
    toasts.info('No Claude session in this tab');
    return;
  }
  try {
    await editorSendSelection({ editor_session: editor.id, claude_session: claude.id });
  } catch (err) {
    toasts.error(err, 'Sending the selection failed');
  }
}

registerAction('palette.open', () => ui.toggleOverlay('palette'));
registerAction('project.switcher', () => ui.toggleOverlay('switcher'));
for (let n = 1; n <= 9; n += 1) registerAction(`project.goto.${n}`, () => gotoProjectIndex(n));
registerAction('inbox.open', () => openInbox());
registerAction('project.next', () => cycleProject(1));
registerAction('project.prev', () => cycleProject(-1));
registerAction('tab.next', () => cycleTab(1));
registerAction('tab.prev', () => cycleTab(-1));
registerAction('session.new', () => {
  if (projects.activeId && layout.get(projects.activeId) === null) void layout.ensure(projects.activeId);
  ui.openSheet('session_new');
});
registerAction('pane.split_right', () => splitFocused('row'));
registerAction('pane.split_down', () => splitFocused('column'));
registerAction('pane.focus_left', () => focusDirection('left'));
registerAction('pane.focus_down', () => focusDirection('down'));
registerAction('pane.focus_up', () => focusDirection('up'));
registerAction('pane.focus_right', () => focusDirection('right'));
registerAction('pane.zoom', () => toggleZoomFocused());
registerAction('pane.close', () => closeFocusedPane());
registerAction('attention.next', () => attentionNext());
registerAction('terminal.search', () => {
  const id = focusedSessionId();
  if (id) terminalUi.searchSession = id;
});
registerAction('terminal.copy', async () => {
  await focusedView()?.copy();
});
registerAction('terminal.paste', async () => {
  await focusedView()?.paste();
});
registerAction('editor.send_selection', () => sendSelection());

/** Toast action of the Linux renderer probe: switches `terminal.renderer`. */
registerAction('terminal.set_renderer', async (args) => {
  const renderer = typeof args?.renderer === 'string' ? args.renderer : 'webgl';
  try {
    await settingsSet({ layer: 'global', path: 'terminal.renderer', value: renderer });
    toasts.info(`Terminal renderer set to ${renderer}`);
  } catch (err) {
    toasts.error(err, 'Changing the renderer failed');
  }
});
