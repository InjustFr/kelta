// Navigation and layout operations behind the shell actions: switching projects, revealing
// sessions, splitting, zooming and closing panes and tabs. Every function works on the stores and
// the pure layout ops; none of them touches a process except the explicit `kill` paths.

import type {
  Layout,
  OpenPaneRequest,
  PaneContent,
  PaneId,
  ProjectId,
  ProjectInfo,
  SessionId,
  SessionInfo,
  SplitDir,
  Tab,
  TabId,
} from '$lib/gen';
import { sessionSpawn } from '$lib/ipc/commands';
import {
  activateTab,
  activeTab,
  allPanes,
  closePaneInLayout,
  closeTab,
  findPane,
  findSession,
  focusPane,
  layoutSessions,
  neighbor,
  pane as makePane,
  paneSession,
  splitPane,
  toggleZoom,
  updateTab,
  type Direction,
  type PaneNode,
} from '$lib/layout';
import { layout, projects, sessions, toasts, ui, work } from '$lib/stores';
import { lampOf, maxAttention } from '$lib/stores/reducers';
import { terminalPool } from '$lib/terminal';

import { phaseNow } from '../views/work/live';
import { maxLamp, type Lamp } from '../views/work/phase';

import { confirms } from './confirm.svelte';

// ---- projects -------------------------------------------------------------------------------

/** Rail order: open projects in list order, Home last. `project.goto.N` follows this order. */
export function railProjects(): ProjectInfo[] {
  const open = projects.list.filter((p) => p.open);
  return [...open.filter((p) => !p.builtin), ...open.filter((p) => p.builtin)];
}

export function activeProjectId(): ProjectId | null {
  return projects.activeId;
}

/** Visible terminal sessions of a project's active tab (views about to be shown). */
export function visibleSessions(projectId: ProjectId): SessionId[] {
  const l = layout.get(projectId);
  const tab = l ? activeTab(l) : null;
  if (!tab) return [];
  return allPanes(tab.root)
    .map((p) => paneSession(p))
    .filter((s): s is SessionId => s !== null);
}

/**
 * Makes `id` the active project. The local state flips synchronously (the shell re-renders in the
 * same task: pooled terminal views are re-parented, nothing is killed or re-attached); the backend
 * confirms afterwards. Records the `kelta.project_switch` performance measure.
 */
export async function activateProject(id: ProjectId): Promise<void> {
  const target = projects.byId(id);
  if (!target) return;
  const alreadyActive = projects.activeId === id && !ui.inboxActive;
  if (alreadyActive) return;
  if (typeof performance !== 'undefined') performance.mark('kelta.switch.start');
  ui.inboxActive = false;
  terminalPool.protect(visibleSessions(id));
  const pending = projects.activate(id);
  void layout.ensure(id).catch(() => {});
  pending.catch((err: unknown) => toasts.error(err, 'Switching project failed'));
  if (typeof performance !== 'undefined') {
    queueMicrotask(() => {
      try {
        performance.measure('kelta.project_switch', 'kelta.switch.start');
      } catch {
        // mark missing (cleared by a test)
      }
    });
  }
  await pending.catch(() => {});
}

export function gotoProjectIndex(n: number): void {
  const target = railProjects()[n - 1];
  if (target) void activateProject(target.id);
}

export function cycleProject(delta: 1 | -1): void {
  const rail = railProjects();
  if (rail.length === 0) return;
  const at = ui.inboxActive ? -1 : rail.findIndex((p) => p.id === projects.activeId);
  const next = rail[(at + delta + rail.length) % rail.length];
  if (next) void activateProject(next.id);
}

export function openInbox(): void {
  ui.inboxActive = true;
}

// ---- focus helpers --------------------------------------------------------------------------

export function currentLayout(projectId: ProjectId | null = projects.activeId): Layout | null {
  return projectId ? layout.get(projectId) : null;
}

export function currentTab(projectId: ProjectId | null = projects.activeId): Tab | null {
  const l = currentLayout(projectId);
  return l ? activeTab(l) : null;
}

export function focusedPane(projectId: ProjectId | null = projects.activeId): PaneNode | null {
  const tab = currentTab(projectId);
  if (!tab) return null;
  const id = tab.focused_pane;
  const hit = id ? findPane(tab.root, id) : null;
  return hit?.pane ?? allPanes(tab.root)[0] ?? null;
}

export function focusedSessionId(projectId: ProjectId | null = projects.activeId): SessionId | null {
  const p = focusedPane(projectId);
  return p ? paneSession(p) : null;
}

export function focusedSession(projectId: ProjectId | null = projects.activeId): SessionInfo | null {
  const id = focusedSessionId(projectId);
  return id ? sessions.get(id) : null;
}

export function focusPaneById(projectId: ProjectId, tabId: TabId, paneId: PaneId): void {
  const l = layout.get(projectId);
  const tab = l?.tabs.find((t) => t.id === tabId);
  if (tab && tab.focused_pane === paneId && l?.active_tab === tabId) return;
  layout.update(projectId, (cur) => focusPane(cur, tabId, paneId));
}

export function focusDirection(direction: Direction): void {
  const projectId = projects.activeId;
  const tab = currentTab();
  const from = focusedPane();
  if (!projectId || !tab || !from) return;
  const target = neighbor(tab.root, from.id, direction);
  if (target) focusPaneById(projectId, tab.id, target);
}

// ---- sessions -------------------------------------------------------------------------------

/**
 * Brings a session into view: activates its project, tab and pane. Background sessions (not shown
 * in any pane) open in a new tab.
 */
export async function revealSession(sessionId: SessionId): Promise<boolean> {
  const s = sessions.get(sessionId);
  if (!s) return false;
  const projectId = s.project_id;
  if (!projects.byId(projectId)) return false;
  try {
    await layout.ensure(projectId);
  } catch (err) {
    toasts.error(err, 'Loading the layout failed');
    return false;
  }
  void activateProject(projectId);
  const l = layout.get(projectId);
  const loc = l ? findSession(l, sessionId) : null;
  if (loc) {
    layout.update(projectId, (cur) => focusPane(cur, loc.tabId, loc.paneId));
  } else {
    layout.open(projectId, {
      content: { kind: 'terminal', session_id: sessionId },
      placement: 'new_tab',
      focus: true,
      tab_title: s.name,
      work_item_id: s.work_item_id,
    });
  }
  return true;
}

// ---- opening content ------------------------------------------------------------------------

/** Opens a pane in a project (activating it) per the placement rules of ARCH §5.1. */
export async function openContent(
  projectId: ProjectId,
  request: Omit<OpenPaneRequest, 'focus' | 'tab_title' | 'work_item_id'> &
    Partial<Pick<OpenPaneRequest, 'focus' | 'tab_title' | 'work_item_id'>>,
): Promise<void> {
  await layout.ensure(projectId);
  void activateProject(projectId);
  layout.open(projectId, {
    focus: true,
    tab_title: null,
    work_item_id: null,
    ...request,
  });
}

// ---- splitting, zoom, close -----------------------------------------------------------------

/** Splits the focused pane; the new pane is a shell in the same working directory. */
export async function splitFocused(dir: SplitDir): Promise<void> {
  const projectId = projects.activeId;
  const tab = currentTab();
  const from = focusedPane();
  if (!projectId || !tab || !from) return;
  const session = focusedSession();
  const project = projects.byId(projectId);
  try {
    const created = await sessionSpawn({
      req: {
        id: null,
        project_id: projectId,
        kind: { type: 'shell' },
        name: null,
        program: null,
        args: [],
        cwd: session?.cwd ?? project?.repos[0]?.path ?? null,
        env: {},
        cols: session?.cols ?? 120,
        rows: session?.rows ?? 40,
        work_item_id: session?.work_item_id ?? null,
        restore: { kind: 'shell_in_cwd' },
        close_on_exit: 'never',
        template_id: null,
      },
    });
    sessions.upsert(created);
    const node = makePane({ kind: 'terminal', session_id: created.id });
    layout.update(projectId, (l) =>
      updateTab(l, tab.id, (t) => ({
        ...t,
        root: splitPane(t.root, from.id, dir, node),
        focused_pane: node.id,
      })),
    );
  } catch (err) {
    toasts.error(err, 'Splitting the pane failed');
  }
}

export function toggleZoomFocused(): void {
  const projectId = projects.activeId;
  const tab = currentTab();
  if (!projectId || !tab) return;
  layout.update(projectId, (l) => toggleZoom(l, tab.id));
}

/** Closes a pane. Terminal sessions keep running as background sessions. */
export function closePaneById(projectId: ProjectId, tabId: TabId, paneId: PaneId): void {
  layout.update(projectId, (l) => closePaneInLayout(l, tabId, paneId));
}

export function closeFocusedPane(): void {
  const projectId = projects.activeId;
  const tab = currentTab();
  const p = focusedPane();
  if (projectId && tab && p) closePaneById(projectId, tab.id, p.id);
}

/** Closes the pane and stops its session. */
export async function closePaneAndStop(projectId: ProjectId, tabId: TabId, paneId: PaneId): Promise<void> {
  const l = layout.get(projectId);
  const tab = l?.tabs.find((t) => t.id === tabId);
  const node = tab ? findPane(tab.root, paneId)?.pane : null;
  const sid = node ? paneSession(node) : null;
  closePaneById(projectId, tabId, paneId);
  if (sid) await sessions.kill(sid).catch((err: unknown) => toasts.error(err, 'Stopping the session failed'));
}

export function cycleTab(delta: 1 | -1): void {
  const projectId = projects.activeId;
  const l = currentLayout();
  if (!projectId || !l || l.tabs.length === 0) return;
  const cur = activeTab(l);
  const at = cur ? l.tabs.findIndex((t) => t.id === cur.id) : 0;
  const next = l.tabs[(at + delta + l.tabs.length) % l.tabs.length];
  if (next) layout.update(projectId, (x) => activateTab(x, next.id));
}

export function selectTab(projectId: ProjectId, tabId: TabId): void {
  layout.update(projectId, (l) => activateTab(l, tabId));
}

/** Sessions of a tab that still have a live process. */
export function liveSessionsOfTab(tab: Tab): SessionInfo[] {
  return allPanes(tab.root)
    .map((p) => paneSession(p))
    .filter((s): s is SessionId => s !== null)
    .map((s) => sessions.get(s))
    .filter((s): s is SessionInfo => s !== null && s.lifecycle === 'live');
}

/**
 * Closes a tab (middle click, tab menu). With live processes it asks first: sessions can keep
 * running in the background or be stopped with the tab.
 */
export async function requestCloseTab(projectId: ProjectId, tabId: TabId): Promise<void> {
  const l = layout.get(projectId);
  const tab = l?.tabs.find((t) => t.id === tabId);
  if (!tab) return;
  const live = liveSessionsOfTab(tab);
  let stop = false;
  if (live.length > 0) {
    const answer = await confirms.ask({
      title: `Close “${tab.title}”?`,
      body: `${live.length} session${live.length === 1 ? ' is' : 's are'} still running. They can keep running in the background (listed in the command palette) or be stopped with the tab.`,
      details: live.map((s) => s.name),
      actions: [
        { id: 'keep', label: 'Close tab, keep sessions', variant: 'primary' },
        { id: 'stop', label: 'Close and stop sessions', variant: 'danger' },
      ],
    });
    if (answer === null) return;
    stop = answer === 'stop';
  }
  const sids = layoutSessionsOfTab(tab);
  layout.update(projectId, (cur) => closeTab(cur, tabId));
  if (stop) {
    await Promise.all(
      sids.map((sid) =>
        sessions.kill(sid).catch((err: unknown) => toasts.error(err, 'Stopping the session failed')),
      ),
    );
  }
}

function layoutSessionsOfTab(tab: Tab): SessionId[] {
  return layoutSessions({ project_id: '', tabs: [tab], active_tab: tab.id, rev: 0 });
}

/** Work item linked to a tab (for the tab header). */
export function workItemOfTab(tab: Tab) {
  return tab.work_item_id ? work.get(tab.work_item_id) : null;
}

// ---- attention ------------------------------------------------------------------------------

/** Lamp of a session; a work item's sessions leave "done" to the item's phase (`review_due`). */
function sessionLamp(s: SessionInfo | null): Lamp {
  if (!s) return 'none';
  if (s.work_item_id && s.attention === 'done') return 'none';
  return s.attention;
}

/** One lamp for a set of sessions (a ticket's work item). */
export function sessionsLamp(ids: (SessionId | null)[]) {
  const tabSessions = ids.filter((s): s is SessionId => s !== null).map((s) => sessions.get(s));
  return lampOf(
    maxAttention(tabSessions.map((s) => s?.attention ?? 'none')),
    tabSessions.some((s) => s?.status === 'working'),
  );
}

/** Tab lamp: its sessions, and for a work tab the item's phase (FLOW §2.3). */
export function tabAttention(tab: Tab): Lamp {
  const lamps = allPanes(tab.root)
    .map((p) => paneSession(p))
    .filter((s): s is SessionId => s !== null)
    .map((s) => sessionLamp(sessions.get(s)));
  const item = tab.work_item_id ? work.get(tab.work_item_id) : null;
  if (item && item.state.kind !== 'finished') lamps.push(phaseNow(item).lamp);
  return maxLamp(lamps);
}

/** Rail tile lamp: the project's sessions folded with its work items' phases. */
export function projectAttention(id: ProjectId): Lamp {
  const lamps = sessions.forProject(id).map(sessionLamp);
  for (const item of work.forProject(id)) if (item.state.kind !== 'finished') lamps.push(phaseNow(item).lamp);
  return maxLamp(lamps);
}

export type { PaneContent };
