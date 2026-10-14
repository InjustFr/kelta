// Navigation and layout operations behind the shell actions: switching projects, revealing
// sessions, splitting, zooming and closing panes and tabs. Every function works on the stores and
// the pure layout ops; none of them touches a process except the explicit `kill` paths.

import { t, tn } from '$lib/i18n/index.svelte';
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
  WorkItem,
} from '$lib/gen';
import { sessionSpawn, windowDetach, workCheckPrs } from '$lib/ipc/commands';
import {
  activateTab,
  activeTab,
  allPanes,
  closePaneInLayout,
  closeTab,
  contentEquals,
  detachPane,
  findContent,
  findPane,
  findSession,
  focusPane,
  findTab,
  layoutSessions,
  neighbor,
  pane as makePane,
  paneSession,
  setDetached,
  splitPane,
  toggleZoom,
  updateTab,
  type Direction,
  type PaneNode,
} from '$lib/layout';
import { layout, projects, sessions, toasts, tools, ui, work, type GlobalView } from '$lib/stores';
import { terminalPool } from '$lib/terminal';

import { phaseNow, unfinishedWork } from '../views/work/live';
import { zoneOf } from '../views/inbox/groups';
import { goToWork } from '../views/work/nav';
import { maxLamp, type Lamp } from '../views/work/phase';

import { confirms } from './confirm.svelte';
import { detached } from './detached';

// ---- projects -------------------------------------------------------------------------------

/** Rail order: open projects in list order, Home last. `project.goto.N` follows this order. */
export function railProjects(): ProjectInfo[] {
  const open = projects.list.filter((p) => p.open);
  return [...open.filter((p) => !p.builtin), ...open.filter((p) => p.builtin)];
}

/**
 * A rail row of an open project: an unfinished work item (with its tab, or none: a click resumes it),
 * a Claude session without a work item (its tab, or none: live in the background), or a folded tab.
 */
export interface RailRow {
  project: ProjectId;
  tab: Tab | null;
  item?: WorkItem;
  session?: SessionInfo;
}

/**
 * A band (#435): its unfinished work items by start time, so a phase change never moves a row; an item
 * waiting on others has a row only while its tab is open (#443); then its Claude sessions without a
 * work item, shown in a tab or live in the background; then every other tab (shells, tools, lists),
 * folded under "n more".
 */
export function bandTabs(projectId: ProjectId): { work: RailRow[]; claude: RailRow[]; more: Tab[] } {
  const l = layout.get(projectId);
  const tabs = l?.tabs ?? [];
  const work = unfinishedWork()
    .filter((w) => w.project_id === projectId)
    .sort((a, b) => a.created_at.localeCompare(b.created_at))
    .flatMap((item) => {
      const tab = tabs.find((tab) => tab.work_item_id === item.id) ?? null;
      const phase = phaseNow(item);
      // In flight with Claude working is the Queue's Claude zone, not Waiting on others (#413).
      const waits = zoneOf(phase.section) === 'waiting' && phase.lamp !== 'working';
      return tab || !waits ? [{ project: projectId, tab, item }] : [];
    });
  const claude = sessions
    .forProject(projectId)
    .filter((s) => s.kind.type === 'claude' && !s.work_item_id)
    .sort((a, b) => a.created_at.localeCompare(b.created_at))
    .flatMap((session) => {
      const loc = l ? findSession(l, session.id) : null;
      if (!loc && session.lifecycle !== 'live') return [];
      return [{ project: projectId, tab: loc && l ? findTab(l, loc.tabId) : null, session }];
    });
  const shown = new Set([...work, ...claude].map((r) => r.tab?.id));
  return { work, claude, more: tabs.filter((tab) => !shown.has(tab.id)) };
}

/** The rail's rows in rail order: each band's work rows, its Claude rows, then its folded tabs. */
export function railRows(): RailRow[] {
  return railProjects().flatMap((p) => {
    const { work, claude, more } = bandTabs(p.id);
    return [...work, ...claude, ...more.map((tab) => ({ project: p.id, tab }))];
  });
}

/** The work rows `project.goto.N` jumps to, across bands. */
export function workRows(): RailRow[] {
  return railRows().filter((r) => r.item);
}

/** A rail row click: a session is revealed, a tab shown, a work item without one resumed (`work_resume`). */
export function openRow(r: RailRow): void {
  if (r.session) void revealSession(r.session.id);
  else if (r.tab) selectRow(r.project, r.tab.id);
  else if (r.item) void goToWork(r.item);
}

/** A row's lamp: its Claude session's, its tab's, else its work item's phase. */
export function rowLamp(r: RailRow): Lamp {
  if (r.session) return sessionLamp(r.session);
  if (r.tab) return tabAttention(r.tab);
  return r.item ? phaseNow(r.item).lamp : 'none';
}

// shortcut: the active project's tool list (Home's in a global view) until #237 slice 6 makes the
// list global; then read that one.
export function railToolsOf(): ProjectId {
  return projects.activeId ?? 'home';
}

/** The tools in config order: `tool.goto.N` opens tool N, the palette lists them. */
export function railTools(): ToolInfo[] {
  return tools.list(railToolsOf());
}

/** Live sessions of a project that no pane shows (tools and plain Claude aside: rows of their own): the dim folded rows. */
export function backgroundSessions(projectId: ProjectId): SessionInfo[] {
  const l = layout.get(projectId);
  if (!l) return [];
  return sessions
    .forProject(projectId)
    .filter(
      (s) =>
        s.lifecycle === 'live' &&
        s.kind.type !== 'tool' &&
        !(s.kind.type === 'claude' && !s.work_item_id) &&
        !findSession(l, s.id),
    );
}

/** Rail label of a tab: `KEY title` for a work tab whose title lacks the key. */
export function rowLabel(tab: Tab): string {
  const key = workItemOfTab(tab)?.ticket?.key;
  // Default titles are stored in English ("Settings"): shown in the interface language.
  const title = t(tab.title);
  return key && !tab.title.startsWith(key) ? `${key} ${title}` : title;
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
  ui.settings = null;
  const alreadyActive = projects.activeId === id && ui.view === null;
  if (alreadyActive) return;
  if (typeof performance !== 'undefined') performance.mark('kelta.switch.start');
  ui.view = null;
  terminalPool.protect(visibleSessions(id));
  const pending = projects.activate(id);
  void layout.ensure(id).catch(() => {});
  pending.catch((err: unknown) => toasts.error(err, t('Switching project failed')));
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

/** `project.goto.N`: work row N, counted across bands. */
export function gotoRailRow(n: number): void {
  const row = workRows()[n - 1];
  if (row) openRow(row);
}

export function cycleProject(delta: 1 | -1): void {
  const rail = railProjects();
  if (rail.length === 0) return;
  // From a global view: next is the first project, previous the last.
  const at = ui.view ? (delta > 0 ? -1 : 0) : rail.findIndex((p) => p.id === projects.activeId);
  const next = rail[(at + delta + rail.length) % rail.length];
  if (next) void activateProject(next.id);
}

export const VIEW_NAMES: Record<GlobalView, string> = {
  get tickets() {
    return t('Tickets');
  },
  get now() {
    return t('Queue');
  },
  get reviews() {
    return t('Pull requests');
  },
};

/** Shows a global view on the rail; the active project stays as it is underneath. `select`: the row
 * key the view lands on (a cross link between Tickets and Reviews). */
export function openView(view: GlobalView, select?: string): void {
  ui.select = select ? { view, key: select } : null;
  ui.settings = null;
  ui.view = view;
  // Merges / closes missed while Kelta was closed or the poll was off (FLOW §3.6).
  if (view === 'now') workCheckPrs({}).catch((err) => console.warn('[kelta] work PR check failed', err));
}

// ---- focus helpers --------------------------------------------------------------------------

/** The project this window works on: its detached tab's (#342), else the active one. */
function here(): ProjectId | null {
  return detached?.project_id ?? projects.activeId;
}

export function currentLayout(projectId: ProjectId | null = here()): Layout | null {
  return projectId ? layout.get(projectId) : null;
}

/** The tab on screen: a detached window's own tab, else the project's active tab. */
export function currentTab(projectId: ProjectId | null = here()): Tab | null {
  const l = currentLayout(projectId);
  if (!l) return null;
  return detached?.project_id === projectId ? findTab(l, detached.tab_id) : activeTab(l);
}

/** Where the user is, for `toasts.where`: the global view, else the project's tab; `null` before layouts load. */
export function viewKey(): string | null {
  if (ui.view) return ui.view;
  const tab = currentTab();
  return tab ? `${here()}/${tab.id}` : null;
}

export function focusedPane(projectId: ProjectId | null = here()): PaneNode | null {
  const tab = currentTab(projectId);
  if (!tab) return null;
  const id = tab.focused_pane;
  const hit = id ? findPane(tab.root, id) : null;
  return hit?.pane ?? allPanes(tab.root)[0] ?? null;
}

export function focusedSessionId(projectId: ProjectId | null = here()): SessionId | null {
  const p = focusedPane(projectId);
  return p ? paneSession(p) : null;
}

export function focusedSession(projectId: ProjectId | null = here()): SessionInfo | null {
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
  if (ui.settings) return; // the workspace under the overlay is hidden
  const projectId = here();
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
    toasts.error(err, t('Loading the layout failed'));
    return false;
  }
  const l = layout.get(projectId);
  const loc = l ? findSession(l, sessionId) : null;
  if (loc && l && findTab(l, loc.tabId)?.detached) {
    // Its window shows it: raise that one (an existing window is unminimized and focused).
    await windowDetach({ project_id: projectId, tab_id: loc.tabId }).catch((err: unknown) =>
      toasts.error(err, t('Opening the window failed')),
    );
    return true;
  }
  void activateProject(projectId);
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

/** A session's workflow runs (#392): a pane split right of its terminal, in its tab; open again = focus it. */
export async function openRuns(sessionId: SessionId): Promise<boolean> {
  const projectId = sessions.get(sessionId)?.project_id;
  if (!projectId || !(await revealSession(sessionId))) return false;
  const l = layout.get(projectId);
  const loc = l ? findSession(l, sessionId) : null;
  if (!l || !loc) return false;
  const content: PaneContent = { kind: 'claude_runs', session_id: sessionId };
  const hit = findContent(l, (c) => contentEquals(c, content));
  // Un-zoom the tab: a zoomed terminal would hide the runs pane.
  const unzoom = (cur: Layout, tabId: TabId) => updateTab(cur, tabId, (t) => ({ ...t, zoomed_pane: null }));
  layout.update(projectId, (cur) => {
    if (hit) return focusPane(unzoom(cur, hit.tabId), hit.tabId, hit.paneId);
    const node = makePane(content);
    const split = updateTab(cur, loc.tabId, (t) => ({
      ...t,
      zoomed_pane: null,
      root: splitPane(t.root, loc.paneId, 'row', node),
    }));
    return focusPane(split, loc.tabId, node.id);
  });
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
  if (projects.activeId && !detached) await activateProject(projects.activeId);
  const projectId = here();
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
    toasts.error(err, t('Splitting the pane failed'));
  }
}

export function toggleZoomFocused(): void {
  if (ui.settings) return;
  const projectId = here();
  const tab = currentTab();
  if (!projectId || !tab) return;
  layout.update(projectId, (l) => toggleZoom(l, tab.id));
}

/** Closes a pane. Terminal sessions keep running as background sessions. */
export function closePaneById(projectId: ProjectId, tabId: TabId, paneId: PaneId): void {
  layout.update(projectId, (l) => closePaneInLayout(l, tabId, paneId));
}

/**
 * Closes a pane (pane ✕, pane menu, `pane.close`). A live session asks first: stop it, or keep it
 * running in the background. Cancel and Escape keep the pane.
 */
export async function requestClosePane(projectId: ProjectId, tabId: TabId, paneId: PaneId): Promise<void> {
  const tab = layout.get(projectId)?.tabs.find((t) => t.id === tabId);
  const node = tab ? findPane(tab.root, paneId)?.pane : null;
  const sid = node ? paneSession(node) : null;
  const session = sid ? sessions.get(sid) : null;
  if (session?.lifecycle === 'live') {
    // No primary action: Enter and ⌘↵ never pick one, Escape cancels.
    const answer = await confirms.ask({
      title: t('Close “{name}”?', { name: session.name }),
      body: t(
        'The process is still running. It can keep running in the background (listed in the command palette) or be killed with the pane.',
      ),
      actions: [
        { id: 'background', label: t('Move to background') },
        { id: 'kill', label: t('Kill process'), variant: 'danger' },
      ],
    });
    if (answer === null) return;
    if (answer === 'kill') return closePaneAndStop(projectId, tabId, paneId);
  }
  closePaneById(projectId, tabId, paneId);
}

export async function closeFocusedPane(): Promise<void> {
  if (ui.settings) {
    ui.settings = null;
    return;
  }
  const projectId = here();
  const tab = currentTab();
  const p = focusedPane();
  if (projectId && tab && p) await requestClosePane(projectId, tab.id, p.id);
}

/** Closes the pane and stops its session. */
export async function closePaneAndStop(projectId: ProjectId, tabId: TabId, paneId: PaneId): Promise<void> {
  const l = layout.get(projectId);
  const tab = l?.tabs.find((t) => t.id === tabId);
  const node = tab ? findPane(tab.root, paneId)?.pane : null;
  const sid = node ? paneSession(node) : null;
  closePaneById(projectId, tabId, paneId);
  if (sid)
    await sessions.kill(sid).catch((err: unknown) => toasts.error(err, t('Stopping the session failed')));
}

// ---- detached windows (#342) ------------------------------------------------------------------

/** Shows a detached tab's window: the layout is saved first (the window reads it), the main window's views let go. */
async function openTabWindow(projectId: ProjectId, tabId: TabId): Promise<void> {
  const l = layout.get(projectId);
  const tab = l ? findTab(l, tabId) : null;
  for (const p of tab ? allPanes(tab.root) : []) {
    const sid = paneSession(p);
    if (sid) terminalPool.release(sid);
  }
  await layout.flush(projectId);
  await windowDetach({ project_id: projectId, tab_id: tabId }).catch((err: unknown) =>
    toasts.error(err, t('Opening the window failed')),
  );
}

/** "Move to new window" on a pane: its tab when alone, else a new tab holding just the pane. */
export async function detachPaneToWindow(projectId: ProjectId, tabId: TabId, paneId: PaneId): Promise<void> {
  let target: TabId | null = null;
  layout.update(projectId, (l) => {
    const r = detachPane(l, { tabId, paneId });
    target = r.tabId;
    return r.layout;
  });
  if (target) await openTabWindow(projectId, target);
}

/** "Move to new window" on a tab, or brings its window to the front when it already has one. */
export async function detachTab(projectId: ProjectId, tabId: TabId): Promise<void> {
  layout.update(projectId, (l) => setDetached(l, tabId, true));
  await openTabWindow(projectId, tabId);
}

/** "Move back to main window": the detached window closes itself when it sees the saved layout. */
export async function attachTab(projectId: ProjectId, tabId: TabId): Promise<void> {
  layout.update(projectId, (l) => setDetached(l, tabId, false));
  await layout.flush(projectId);
}

/** `pane.detach`: the focused pane to a new window; in a detached window, its tab back to the main one. */
export async function toggleFocusedPaneWindow(): Promise<void> {
  if (detached) return attachTab(detached.project_id, detached.tab_id);
  if (ui.settings) return;
  const projectId = here();
  const tab = currentTab();
  const p = focusedPane();
  if (projectId && tab && p) await detachPaneToWindow(projectId, tab.id, p.id);
}

/** `tab.next/prev`: walks the rail rows that show a tab, across bands; from a global view, next is the first. */
export function cycleTab(delta: 1 | -1): void {
  // A tab shown by two rows (a plain Claude split into a work tab) is walked once.
  const rows = railRows()
    .flatMap((r) => (r.tab ? [{ project: r.project, tab: r.tab }] : []))
    .filter((r, i, all) => all.findIndex((x) => x.tab.id === r.tab.id) === i);
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
      title: t('Close “{name}”?', { name: tab.title }),
      body: tn(
        '{n} session is still running. They can keep running in the background (listed in the command palette) or be stopped with the tab.',
        '{n} sessions are still running. They can keep running in the background (listed in the command palette) or be stopped with the tab.',
        live.length,
      ),
      details: live.map((s) => s.name),
      actions: [
        { id: 'keep', label: t('Close tab, keep sessions'), variant: 'primary' },
        { id: 'stop', label: t('Close and stop sessions'), variant: 'danger' },
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
        sessions.kill(sid).catch((err: unknown) => toasts.error(err, t('Stopping the session failed'))),
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

export type { PaneContent };
