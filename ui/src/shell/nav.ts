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
  ToolInfo,
} from '$lib/gen';
import { sessionSpawn, workCheckPrs } from '$lib/ipc/commands';
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
import { webTools } from '$lib/plugin-host/web.svelte';
import { layout, projects, sessions, toasts, tools, ui, work, type GlobalView } from '$lib/stores';
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

/** A rail row: one tab of an open project. */
export interface RailRow {
  project: ProjectId;
  tab: Tab;
}

/** The rail's rows: every non-tool tab of every open project, in rail order then tab order. */
export function railRows(lines = toolSubLines()): RailRow[] {
  return railProjects().flatMap((p) =>
    (layout.get(p.id)?.tabs ?? [])
      .filter((tab) => !isToolTab(tab, lines))
      .map((tab) => ({ project: p.id, tab })),
  );
}

// shortcut: the active project's tool list (Home's in a global view) until #237 slice 6 makes the
// list global; then read that one.
export function railToolsOf(): ProjectId {
  return projects.activeId ?? 'home';
}

/** The rail's Tools section, in config order; `tool.goto.N` opens tool N. */
export function railTools(): ToolInfo[] {
  return tools.list(railToolsOf());
}

/** The tool a pane shows: a live tool session's or web instance's tool id. */
function livePaneTool(p: PaneNode): string | null {
  if (p.content.kind === 'web') {
    // webTools is in-memory: after a restart a persisted web tab has no instance, so it is a closable row.
    const w = webTools[p.content.tool_instance_id];
    return w && !w.exited ? w.toolId : null;
  }
  const id = paneSession(p);
  const s = id ? sessions.get(id) : null;
  // A dead tool session is no tool instance: its tab falls back to a closable row in its band.
  return s?.kind.type === 'tool' && s.lifecycle === 'live' ? s.kind.tool_id : null;
}

/** A tool pane of an open project, listed under Tools: a sub-line, or a `user` tool's own row. */
export interface ToolInstance {
  project: ProjectId;
  tab: Tab;
  pane: PaneNode;
  tool: string;
}

/**
 * Every tool instance the rail's Tools section stands for, by pane id, in rail then tab order: the one
 * definition behind the rail, `isToolTab` and `liveToolPane`. Components keep it in a `$derived`.
 */
export function toolSubLines(): Map<PaneId, ToolInstance> {
  const home = projects.home?.id;
  const lines = new Map<PaneId, ToolInstance>();
  for (const p of railProjects())
    for (const tab of layout.get(p.id)?.tabs ?? [])
      for (const pane of allPanes(tab.root)) {
        const id = livePaneTool(pane);
        const tool = id ? tools.find(id) : undefined;
        // A tool the config no longer knows is none; a `user` tool lives in Home, elsewhere its tab is a band row.
        if (tool && (p.id === home || tool.scope !== 'user'))
          lines.set(pane.id, { project: p.id, tab, pane, tool: tool.id });
      }
  return lines;
}

/** A tab made only of tool panes: the rail lists it under Tools, not in its project's band. */
export function isToolTab(tab: Tab, lines = toolSubLines()): boolean {
  const panes = allPanes(tab.root);
  return panes.length > 0 && panes.every((p) => lines.get(p.id)?.tab.id === tab.id);
}

/** Live sessions of a project that no pane shows (tools aside): the dim rows at the end of its band. */
export function backgroundSessions(projectId: ProjectId): SessionInfo[] {
  const l = layout.get(projectId);
  if (!l) return [];
  return sessions
    .forProject(projectId)
    .filter((s) => s.lifecycle === 'live' && s.kind.type !== 'tool' && !findSession(l, s.id));
}

/** Rail label of a tab: `KEY title` for a work tab whose title lacks the key. */
export function rowLabel(tab: Tab): string {
  const key = workItemOfTab(tab)?.ticket?.key;
  return key && !tab.title.startsWith(key) ? `${key} ${tab.title}` : tab.title;
}

/** A rail row click: the project (leaving a global view), then the tab. */
export function selectRow(project: ProjectId, tabId: TabId): void {
  void activateProject(project);
  selectTab(project, tabId);
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
 * Makes `id` the active project, leaving a global view: the one way out of Tickets, Now and Reviews
 * (a no-op when `id` is already shown, so callers can always call it). The local state flips synchronously (the shell re-renders in the
 * same task: pooled terminal views are re-parented, nothing is killed or re-attached); the backend
 * confirms afterwards. Records the `kelta.project_switch` performance measure.
 */
export async function activateProject(id: ProjectId): Promise<void> {
  const target = projects.byId(id);
  if (!target) return;
  const alreadyActive = projects.activeId === id && ui.view === null;
  if (alreadyActive) return;
  if (typeof performance !== 'undefined') performance.mark('kelta.switch.start');
  ui.view = null;
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

/** `project.goto.N`: rail row N, counted across bands (tool tabs and background rows excluded). */
export function gotoRailRow(n: number): void {
  const row = railRows()[n - 1];
  if (row) selectRow(row.project, row.tab.id);
}

export function cycleProject(delta: 1 | -1): void {
  const rail = railProjects();
  if (rail.length === 0) return;
  // From a global view: next is the first project, previous the last.
  const at = ui.view ? (delta > 0 ? -1 : 0) : rail.findIndex((p) => p.id === projects.activeId);
  const next = rail[(at + delta + rail.length) % rail.length];
  if (next) void activateProject(next.id);
}

export const VIEW_NAMES: Record<GlobalView, string> = { tickets: 'Tickets', now: 'Now', reviews: 'Reviews' };

/** Shows a global view on the rail; the active project stays as it is underneath. `select`: the row
 * key the view lands on (a cross link between Tickets and Reviews). */
export function openView(view: GlobalView, select?: string): void {
  ui.select = select ? { view, key: select } : null;
  ui.view = view;
  // Merges / closes missed while Kelta was closed or the poll was off (FLOW §3.6).
  if (view === 'now') workCheckPrs({}).catch((err) => console.warn('[kelta] work PR check failed', err));
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
  // From a global view the split lands in the active project: show it first.
  if (projects.activeId) await activateProject(projects.activeId);
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

/** `tab.next/prev`: walks the rail rows across bands; from a global view, next is the first row. */
export function cycleTab(delta: 1 | -1): void {
  const rows = railRows();
  if (rows.length === 0) return;
  const cur = ui.view ? null : currentTab();
  const at = cur ? rows.findIndex((r) => r.project === projects.activeId && r.tab.id === cur.id) : -1;
  const next = rows[((at < 0 ? (delta > 0 ? -1 : 0) : at) + delta + rows.length) % rows.length];
  if (next) selectRow(next.project, next.tab.id);
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

export function layoutSessionsOfTab(tab: Tab): SessionId[] {
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

/** Lamp of a running PTY tool: its sessions' attention, else a plain activity dot; `none` when not running. */
export function toolLamp(toolId: string): Lamp {
  const live = sessions.all.filter(
    (s) => s.kind.type === 'tool' && s.kind.tool_id === toolId && s.lifecycle === 'live',
  );
  if (live.length === 0) return 'none';
  const lvl = lampOf(
    maxAttention(live.map((s) => s.attention)),
    live.some((s) => s.status === 'working'),
  );
  return lvl === 'none' ? 'activity' : lvl;
}

export type { PaneContent };
