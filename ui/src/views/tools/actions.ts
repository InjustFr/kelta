// `tools.open`: with `{tool_id}` opens that tool where the user is (the shown project and its focused
// work item, else Home), asking for a project when the tool needs one; `{pick_project: true}` always
// asks. Without `tool_id` it shows the tool picker.

import type { Placement, ProjectId, TemplateCtx, ToolCheck, ToolInfo, WorkItemId } from '$lib/gen';
import { registerAction } from '$lib/actions';
import { findSession, focusPane, type PaneLocation } from '$lib/layout';
import { toolCheck, toolOpen } from '$lib/ipc/commands';
import { rememberWebTool } from '$lib/plugin-host/web.svelte';
import { layout, projects, sessions, toasts, tools, ui } from '$lib/stores';
import { activateProject } from '../../shell/nav';
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
 * Opens a tool with its placement; the backend opens the pane (PTY session or web pane). A running
 * PTY instance of the same work item (`ctx.work_item_id`) gets focus instead of a second copy (web
 * tools reopen). Returns true when opened or focused, the failed check when the open failed because
 * the binary is missing, false otherwise (toasted). Checks only after a failed open.
 */
export async function openTool(
  projectId: ProjectId,
  tool: Pick<ToolInfo, 'id' | 'label'> & { placement?: Placement },
  ctx: Partial<TemplateCtx>,
): Promise<true | false | ToolCheck> {
  const at = liveToolPane(projectId, tool.id, ctx.work_item_id ?? null);
  if (at) {
    layout.update(projectId, (l) => focusPane(l, at.tabId, at.paneId));
    return true;
  }
  try {
    const handle = await toolOpen({
      project_id: projectId,
      tool_id: tool.id,
      ctx: { ...EMPTY_CTX, ...ctx },
      placement: tool.placement ?? 'split_right',
    });
    rememberWebTool(handle, { toolId: tool.id, label: tool.label, projectId });
    return true;
  } catch (e) {
    const check = await toolCheck({ tool_id: tool.id }).catch(() => null);
    if (check && !check.installed) return check;
    toasts.error(e, `Open ${tool.label}`);
    return false;
  }
}

/** The live PTY instance of `toolId` in this work item (`null` = no work item), if shown. */
export function liveToolPane(
  projectId: ProjectId,
  toolId: string,
  workItemId: WorkItemId | null,
): PaneLocation | null {
  const live = sessions
    .forProject(projectId)
    .find(
      (s) =>
        s.kind.type === 'tool' &&
        s.kind.tool_id === toolId &&
        s.lifecycle === 'live' &&
        (s.work_item_id ?? null) === workItemId,
    );
  const l = layout.get(projectId);
  return live && l ? findSession(l, live.id) : null;
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
  const info = tools.list(projects.activeId).find((t) => t.id === toolId);
  const tool = info ?? { id: toolId, label: toolId };
  let projectId = here ?? home;
  let workItem = here ? focusedWorkItem() : null;
  if (args?.pick_project === true || (info?.needs_project && !here)) {
    const picked = await ui.pickProject(`Open ${tool.label} for project…`);
    if (!picked) return;
    try {
      if (!projects.byId(picked)?.open) await projects.open(picked);
    } catch (e) {
      toasts.error(e, 'Opening the project failed');
      return;
    }
    projectId = picked;
    workItem = null;
  }
  await activateProject(projectId);
  const ctx = { work_item_id: workItem, ...(args?.ctx as Partial<TemplateCtx>) };
  const r = await openTool(projectId, tool, ctx);
  // A missing binary opens the picker on that tool with its install hint and "Check again".
  if (typeof r === 'object')
    ui.openSheet('tool_picker', { projectId, query: toolId, checks: { [toolId]: r } });
});
