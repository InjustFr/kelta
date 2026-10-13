// `tools.open`: with `{tool_id}` opens that tool where the user is (the shown project and its focused
// work item, else Home), asking for a project when the tool needs one; `{pick_project: true}` always
// asks. Without `tool_id` it shows the tool picker. A terminal tool opens zoomed in the current tab
// (#412); pressed again while it is in front it hides (`tools.hide`, also `Esc` `Esc`), and its key
// brings it back while it runs. A web tool pressed again in front goes back (`nav.back`), one that
// takes a project asks for another project. `tool.goto.N` opens tool N in config order.

import { t } from '$lib/i18n/index.svelte';
import type {
  Placement,
  ProjectId,
  SessionId,
  TemplateCtx,
  ToolCheck,
  ToolInfo,
  ToolScope,
  WorkItemId,
} from '$lib/gen';
import { dispatch, registerAction } from '$lib/actions';
import {
  activeTab,
  allPanes,
  closePaneInLayout,
  findSession,
  focusPane,
  neighbor,
  openPane,
  paneSession,
  updateTab,
  type PaneLocation,
} from '$lib/layout';
import { toolCheck, toolOpen } from '$lib/ipc/commands';
import { rememberWebTool, webTools } from '$lib/plugin-host/web.svelte';
import { layout, projects, sessions, toasts, tools, ui } from '$lib/stores';
import { flushJump, navStep } from '../../shell/jumplist';
import { activateProject, currentTab, focusedPane, railTools } from '../../shell/nav';
import { focusedWorkItem } from '../work/fixloop.svelte';

export const EMPTY_CTX: TemplateCtx = {
  repo_id: null,
  cwd: null,
  session_id: null,
  work_item_id: null,
  ticket: null,
  review: null,
  extra: {},
};

type OpenableTool = Pick<ToolInfo, 'id' | 'label'> & {
  placement?: Placement;
  scope?: ToolScope;
  kind?: ToolInfo['kind'];
};

/**
 * Opens a tool per its `kind` and `scope`. A terminal (`pty`) tool opens as a zoomed pane in the
 * current tab of `projectId`: one instance per work item (`ctx.work_item_id`) for `work` (default),
 * one per project for `project` and `user`; a running one (shown or hidden) comes to the current tab
 * instead of a second copy. A web tool keeps its own place: `work` at its placement, `project` as its
 * own tab, `user` as its own tab in Home; a shown one gets focus. Activates the host first. Returns
 * true when opened or focused, the failed check when the open failed because the binary is missing,
 * false otherwise (toasted). Checks only after a failed open.
 */
export async function openTool(
  projectId: ProjectId,
  tool: OpenableTool,
  ctx: Partial<TemplateCtx>,
): Promise<true | false | ToolCheck> {
  const scope = tool.scope ?? 'work';
  const pty = tool.kind === 'pty';
  const host = scope === 'user' && !pty ? (projects.home?.id ?? 'home') : projectId;
  const workItem = scope === 'work' ? (ctx.work_item_id ?? null) : null;
  await activateProject(host);
  if (pty) {
    const sid = liveToolSession(host, tool.id, workItem, scope);
    if (sid) {
      showZoomed(host, sid, tool.label, workItem);
      return true;
    }
    await layout.flush(host); // the backend splits the pane focused here, not a stale one
  } else {
    const at = liveToolPane(host, tool.id, workItem, scope);
    if (at) {
      layout.update(host, (l) => focusPane(l, at.tabId, at.paneId));
      return true;
    }
  }
  try {
    const handle = await toolOpen({
      project_id: host,
      tool_id: tool.id,
      ctx: { ...EMPTY_CTX, ...ctx, work_item_id: workItem },
      placement: pty ? 'split_right' : scope === 'work' ? (tool.placement ?? 'split_right') : 'new_tab',
    });
    if (handle.kind === 'pty') {
      // shortcut: zooms the backend's new pane from here; a `layout.changed` landing after this undoes the
      // zoom (the tool shows as a split). Upgrade when `OpenPaneRequest` can ask for a zoomed pane.
      const l = layout.get(host);
      if (!l || !findSession(l, handle.session_id)) await layout.load(host);
      showZoomed(host, handle.session_id, tool.label, workItem);
    }
    rememberWebTool(handle, { toolId: tool.id, label: tool.label, projectId: host });
    return true;
  } catch (e) {
    const check = await toolCheck({ tool_id: tool.id }).catch(() => null);
    if (check && !check.installed) return check;
    toasts.error(e, `Open ${tool.label}`);
    return false;
  }
}

/**
 * The running session of terminal tool `toolId` in `projectId`, shown or not: of this work item
 * (`null` = none) for a `work` tool, any for a `project` or `user` one. A shown one wins.
 */
export function liveToolSession(
  projectId: ProjectId,
  toolId: string,
  workItemId: WorkItemId | null,
  scope: ToolScope = 'work',
): SessionId | null {
  const live = sessions
    .forProject(projectId)
    .filter(
      (s) =>
        s.lifecycle === 'live' &&
        s.kind.type === 'tool' &&
        s.kind.tool_id === toolId &&
        (scope !== 'work' || (s.work_item_id ?? null) === workItemId),
    );
  const l = layout.get(projectId);
  return (live.find((s) => l && findSession(l, s.id)) ?? live[0])?.id ?? null;
}

/** Shows `sid` zoomed and focused in the current tab: moved there from another pane, or split off the focused one. */
function showZoomed(projectId: ProjectId, sid: SessionId, title: string, workItem: WorkItemId | null): void {
  layout.update(projectId, (l) => {
    const tab = activeTab(l);
    let at = findSession(l, sid);
    if (at && at.tabId === tab?.id) l = focusPane(l, at.tabId, at.paneId);
    else {
      const content = { kind: 'terminal' as const, session_id: sid };
      const req = { content, placement: 'split_right' as const, focus: true, tab_title: title, work_item_id: workItem };
      ({ layout: l, location: at } = openPane(l, req));
    }
    const { tabId, paneId } = at;
    return updateTab(l, tabId, (x) => ({ ...x, zoomed_pane: paneId }));
  });
}

/**
 * Hides the terminal tool in front (of `toolId`, any when omitted): its pane closes, its session keeps
 * running, and the pane it split from takes the focus back. False when no tool pane is in front.
 */
export function hideFrontTool(toolId?: string): boolean {
  const projectId = ui.view === null ? projects.activeId : null;
  const tab = projectId ? currentTab(projectId) : null;
  const pane = projectId ? focusedPane(projectId) : null;
  const sid = pane ? paneSession(pane) : null;
  const s = sid ? sessions.get(sid) : null;
  if (!projectId || !tab || !pane || s?.kind.type !== 'tool' || (toolId && s.kind.tool_id !== toolId))
    return false;
  const back = neighbor(tab.root, pane.id, 'left') ?? allPanes(tab.root).find((p) => p.id !== pane.id)?.id;
  layout.update(projectId, (l) => {
    const next = closePaneInLayout(l, tab.id, pane.id);
    return back ? focusPane(next, tab.id, back) : next;
  });
  return true;
}

/**
 * The shown instance of web tool `toolId` in `projectId`: of this work item (`null` = none) for a
 * `work` tool, any for a `project` or `user` one (the caller passes Home for `user`).
 */
export function liveToolPane(
  projectId: ProjectId,
  toolId: string,
  workItemId: WorkItemId | null,
  scope: ToolScope = 'work',
): PaneLocation | null {
  for (const tab of layout.get(projectId)?.tabs ?? [])
    for (const p of allPanes(tab.root)) {
      // webTools is in-memory: after a restart a persisted web tab has no instance.
      const w = p.content.kind === 'web' ? webTools[p.content.tool_instance_id] : undefined;
      // A web instance carries no work item: its tab's stands in.
      if (w && !w.exited && w.toolId === toolId && (scope !== 'work' || (tab.work_item_id ?? null) === workItemId))
        return { tabId: tab.id, paneId: p.id };
    }
  return null;
}

registerAction('tools.hide', () => void hideFrontTool());

registerAction('tools.open', async (args) => {
  const toolId = typeof args?.tool_id === 'string' ? args.tool_id : null;
  if (!projects.activeId || !toolId) {
    ui.openSheet('tool_picker');
    return;
  }
  const home = projects.home?.id ?? 'home';
  // A global view (Tickets, Now, Reviews) hides the active project: the tool opens in Home there.
  const here = ui.view === null && projects.activeId !== home ? projects.activeId : null;
  const info = tools.list(projects.activeId).find((t) => t.id === toolId) ?? tools.find(toolId);
  const tool = info ?? { id: toolId, label: toolId };
  const scope = info?.scope ?? 'work';
  const pty = info?.kind === 'pty';
  // The tool's key toggles a terminal tool: in front it hides.
  if (pty && args?.pick_project !== true && hideFrontTool(toolId)) return;
  // The host, decided before anything switches: Home for a `user` web tool, else where you are.
  let projectId = scope === 'user' && !pty ? home : (here ?? home);
  let workItem = here ? focusedWorkItem() : null;
  // A web tool in front: the pane this open would focus is the focused one.
  const ctxItem = (args?.ctx as Partial<TemplateCtx> | undefined)?.work_item_id;
  const at = !pty && ui.view === null ? liveToolPane(projectId, toolId, ctxItem ?? workItem, scope) : null;
  const inFront = at !== null && projects.activeId === projectId && focusedPane()?.id === at.paneId;
  const takesProject = scope !== 'user' && !!info?.needs_project;
  if (inFront && !takesProject && args?.pick_project !== true) {
    navStep(-1);
    return;
  }
  flushJump(); // the place this open leaves is a stop on the way back
  const wantsProject = takesProject || scope === 'project';
  if (args?.pick_project === true || (wantsProject && (!here || inFront))) {
    const picked = await ui.pickProject(t('Open {name} for project…', { name: tool.label }));
    if (!picked) return;
    try {
      if (!projects.byId(picked)?.open) await projects.open(picked);
    } catch (e) {
      toasts.error(e, t('Opening the project failed'));
      return;
    }
    projectId = picked;
    workItem = null;
  }
  const ctx = { work_item_id: workItem, ...(args?.ctx as Partial<TemplateCtx>) };
  const r = await openTool(projectId, tool, ctx);
  // A missing binary opens the picker on that tool with its install hint and "Check again".
  if (typeof r === 'object')
    ui.openSheet('tool_picker', { projectId, query: toolId, checks: { [toolId]: r } });
});

for (let n = 1; n <= 9; n += 1)
  registerAction(`tool.goto.${n}`, async () => {
    const t = railTools()[n - 1];
    if (t) await dispatch('tools.open', { tool_id: t.id });
  });
