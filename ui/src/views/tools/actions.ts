// `tools.open`: with `{tool_id}` opens that tool in the active project, else shows the tool picker.

import type { Placement, ProjectId, TemplateCtx, ToolCheck, ToolInfo } from '$lib/gen';
import { registerAction } from '$lib/actions';
import { findSession, focusPane } from '$lib/layout';
import { toolCheck, toolOpen } from '$lib/ipc/commands';
import { rememberWebTool } from '$lib/plugin-host/web.svelte';
import { layout, projects, sessions, toasts, tools, ui } from '$lib/stores';

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
 * Opens a tool; the backend opens the pane (PTY session or web pane). Returns true when opened,
 * the failed check when the open failed because the binary is missing, false otherwise (toasted).
 * Checks only after a failed open: `tool_check` has no project, so it may see another layer's def.
 */
export async function openTool(
  projectId: ProjectId,
  tool: Pick<ToolInfo, 'id' | 'label'>,
  ctx: Partial<TemplateCtx> = {},
  placement: Placement = 'new_tab',
): Promise<true | false | ToolCheck> {
  try {
    const handle = await toolOpen({
      project_id: projectId,
      tool_id: tool.id,
      ctx: { ...EMPTY_CTX, ...ctx },
      placement,
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

registerAction('tools.open', async (args) => {
  const projectId = projects.activeId;
  const toolId = typeof args?.tool_id === 'string' ? args.tool_id : null;
  if (!projectId || !toolId) {
    ui.openSheet('tool_picker');
    return;
  }
  const info = tools.list(projectId).find((t) => t.id === toolId);
  // An embedded tool that is already running gets focus instead of a second copy.
  // shortcut: pty tools only; a web tool opens again, track its pane when that matters.
  const live = sessions
    .forProject(projectId)
    .find((s) => s.kind.type === 'tool' && s.kind.tool_id === toolId && s.lifecycle === 'live');
  const at = live && layout.get(projectId) ? findSession(layout.get(projectId)!, live.id) : null;
  if (at) {
    layout.update(projectId, (l) => focusPane(l, at.tabId, at.paneId));
    return;
  }
  const r = await openTool(
    projectId,
    { id: toolId, label: info?.label ?? toolId },
    (args?.ctx ?? {}) as Partial<TemplateCtx>,
    'split_right',
  );
  // A missing binary opens the picker on that tool with its install hint and "Check again".
  if (typeof r === 'object')
    ui.openSheet('tool_picker', { projectId, query: toolId, checks: { [toolId]: r } });
});
