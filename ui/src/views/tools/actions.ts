// `tools.open`: with `{tool_id}` opens that tool where the user is (the shown project and its focused
// work item, else Home), asking for a project when the tool needs one; `{pick_project: true}` always
// asks. Without `tool_id` it shows the tool picker. Pressed again while the tool is in front, a tool
// with no project argument goes back (`nav.back`), one that takes a project asks for another project.
// `tool.goto.N` opens tool N of the rail's Tools section.

import { t } from '$lib/i18n/index.svelte';
import type { Placement, ProjectId, TemplateCtx, ToolCheck, ToolInfo, ToolScope, WorkItemId } from '$lib/gen';
import { dispatch, registerAction } from '$lib/actions';
import { focusPane, paneSession, type PaneLocation } from '$lib/layout';
import { toolCheck, toolOpen } from '$lib/ipc/commands';
import { rememberWebTool } from '$lib/plugin-host/web.svelte';
import { layout, projects, sessions, toasts, tools, ui } from '$lib/stores';
import { flushJump, navStep } from '../../shell/jumplist';
import { activateProject, focusedPane, railTools, toolSubLines } from '../../shell/nav';
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

/**
 * Opens a tool in its host per its `scope`: `work` (default) at its placement in `projectId`, one
 * instance per work item (`ctx.work_item_id`); `project` as its own tab, one per project; `user` as
 * its own tab in Home, one for the app. A shown instance (PTY or web) gets focus instead of a second
 * copy. Activates the host first. Returns true when opened or focused, the failed
 * check when the open failed because the binary is missing, false otherwise (toasted). Checks only
 * after a failed open.
 */
export async function openTool(
  projectId: ProjectId,
  tool: Pick<ToolInfo, 'id' | 'label'> & { placement?: Placement; scope?: ToolScope },
  ctx: Partial<TemplateCtx>,
): Promise<true | false | ToolCheck> {
  const scope = tool.scope ?? 'work';
  const host = scope === 'user' ? (projects.home?.id ?? 'home') : projectId;
  const workItem = scope === 'work' ? (ctx.work_item_id ?? null) : null;
  await activateProject(host);
  const at = liveToolPane(host, tool.id, workItem, scope);
  if (at) {
    layout.update(host, (l) => focusPane(l, at.tabId, at.paneId));
    return true;
  }
  try {
    const handle = await toolOpen({
      project_id: host,
      tool_id: tool.id,
      ctx: { ...EMPTY_CTX, ...ctx, work_item_id: workItem },
      placement: scope === 'work' ? (tool.placement ?? 'split_right') : 'new_tab',
    });
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
 * The shown instance of `toolId` in `projectId` (a Tools sub-line, PTY or web): of this work item
 * (`null` = none) for a `work` tool, any for a `project` or `user` one (the caller passes Home for `user`).
 */
export function liveToolPane(
  projectId: ProjectId,
  toolId: string,
  workItemId: WorkItemId | null,
  scope: ToolScope = 'work',
): PaneLocation | null {
  for (const x of toolSubLines().values()) {
    if (x.project !== projectId || x.tool !== toolId) continue;
    const sid = paneSession(x.pane);
    // A web instance carries no work item: its tab's stands in.
    const item = sid ? (sessions.get(sid)?.work_item_id ?? null) : (x.tab.work_item_id ?? null);
    if (scope !== 'work' || item === workItemId) return { tabId: x.tab.id, paneId: x.pane.id };
  }
  return null;
}

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
  // The host, decided before anything switches: Home for a `user` tool, else where you are.
  let projectId = scope === 'user' ? home : (here ?? home);
  let workItem = here ? focusedWorkItem() : null;
  // In front: the pane this open would focus is the focused one.
  const ctxItem = (args?.ctx as Partial<TemplateCtx> | undefined)?.work_item_id;
  const at = ui.view === null ? liveToolPane(projectId, toolId, ctxItem ?? workItem, scope) : null;
  const inFront = at !== null && projects.activeId === projectId && focusedPane()?.id === at.paneId;
  const takesProject = scope !== 'user' && !!info?.needs_project;
  if (inFront && !takesProject && args?.pick_project !== true && args?.focus_only !== true) {
    navStep(-1);
    return;
  }
  flushJump(); // the place this open leaves is a stop on the way back
  const wantsProject = takesProject || scope === 'project';
  if (args?.pick_project === true || (wantsProject && (!here || (inFront && args?.focus_only !== true)))) {
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
