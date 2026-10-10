// Action handlers owned by the shell (BUILD_PLAN §2.4): palette.*, project.*, inbox.open, tab.*,
// session.new, pane.*, attention.*, nav.*, terminal.*, editor.send_selection, editor.quickfix_claude, and the
// terminal file links. Loaded eagerly by main.ts.

import { registerAction } from '$lib/actions';
import type { SessionId, SessionInfo, Tab } from '$lib/gen';
import { editorOpen, editorQuickfix, editorSendSelection, settingsSet } from '$lib/ipc/commands';
import { allPanes, findSession, findTab, paneSession } from '$lib/layout';
import { layout, projects, sessions, toasts, ui, work } from '$lib/stores';
import { terminalPool } from '$lib/terminal';
import { terminalUi } from '$lib/terminal/ui.svelte';
import type { TerminalView } from '$lib/terminal/view';

import {
  closeFocusedPane,
  currentTab,
  cycleProject,
  cycleTab,
  focusDirection,
  focusedSession,
  focusedSessionId,
  gotoProjectIndex,
  liveSessionsOfTab,
  openInbox,
  revealSession,
  splitFocused,
  toggleZoomFocused,
} from './nav';
import { navStep } from './jumplist';

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
  const claude = claudeOf(tab);
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

function claudeOf(tab: Tab): SessionInfo | null {
  return (
    allPanes(tab.root)
      .map((p) => paneSession(p))
      .map((id) => (id ? sessions.get(id) : null))
      .find((s) => s?.kind.type === 'claude') ?? null
  );
}

/** The nvim a session's files open in: its work item's, else its tab's (never another project's). */
export function editorFor(s: SessionInfo): SessionInfo | null {
  if (s.work_item_id) {
    const item = s.work_item_id;
    return (
      sessions.all.find(
        (e) => e.work_item_id === item && e.kind.type === 'editor' && e.lifecycle === 'live',
      ) ?? null
    );
  }
  const l = layout.get(s.project_id);
  const loc = l ? findSession(l, s.id) : null;
  const tab = l && loc ? findTab(l, loc.tabId) : null;
  return (tab && liveSessionsOfTab(tab).find((e) => e.kind.type === 'editor')) ?? null;
}

/** Where a terminal's relative `path:line` links resolve: its cwd (OSC 7), then the worktree root. */
export function fileRoots(id: SessionId): string[] {
  const s = sessions.get(id);
  if (!s) return [];
  const worktree = s.work_item_id ? work.get(s.work_item_id)?.worktree : undefined;
  return worktree ? [s.cwd, worktree] : [s.cwd];
}

/** Cmd/Ctrl-click on a terminal file link; Shift also moves the focus to nvim. */
export async function openFileLink(
  id: SessionId,
  path: string,
  line: number,
  focusEditor: boolean,
): Promise<void> {
  const s = sessions.get(id);
  const editor = s ? editorFor(s) : null;
  if (!editor) {
    toasts.info('No nvim in this tab to open the file');
    return;
  }
  try {
    await editorOpen({ target: { kind: 'session', id: editor.id }, path, line });
    if (focusEditor) await revealSession(editor.id);
  } catch (err) {
    toasts.error(err, 'Opening the file in nvim failed');
  }
}

/** Palette "Quickfix: files Claude touched": the tab's Claude files into its nvim's quickfix list. */
async function quickfixClaude(): Promise<void> {
  const tab = currentTab();
  const claude = tab ? claudeOf(tab) : null;
  const editor = claude ? editorFor(claude) : null;
  if (!claude || !editor) {
    toasts.info('Focus a tab with Claude and nvim first');
    return;
  }
  const files = claude.claude?.files_touched ?? [];
  if (files.length === 0) {
    toasts.info('Claude has not touched any file yet');
    return;
  }
  try {
    await editorQuickfix({ target: { kind: 'session', id: editor.id }, files });
    toasts.info(
      `Quickfix list: ${files.length} file${files.length === 1 ? '' : 's'} Claude touched (]q / [q)`,
    );
  } catch (err) {
    toasts.error(err, 'Filling the quickfix list failed');
  }
}

registerAction('palette.open', () => ui.toggleOverlay('palette'));
registerAction('project.switcher', () => ui.toggleOverlay('switcher'));
for (let n = 1; n <= 9; n += 1) registerAction(`project.goto.${n}`, () => gotoProjectIndex(n));
registerAction('inbox.open', () => openInbox());
registerAction('toast.run_last', async () => {
  const last = toasts.lastActionable;
  if (last) await toasts.run(last.id);
});
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
// Mod+J / Mod+Shift+J walk the jump queue (ticket #136); loaded on first use.
registerAction('attention.next', async () => (await import('../views/inbox/now')).nextWaiting(1));
registerAction('attention.prev', async () => (await import('../views/inbox/now')).nextWaiting(-1));
registerAction('nav.back', () => navStep(-1));
registerAction('nav.forward', () => navStep(1));
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
registerAction('editor.quickfix_claude', () => quickfixClaude());

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
