// `tools.open`: with `{tool_id}` opens that tool in the active project, else shows the tool picker.

import type { ProjectId, TemplateCtx, ToolCheck, ToolInfo } from '$lib/gen';
import { registerAction } from '$lib/actions';
import { toolCheck, toolOpen } from '$lib/ipc/commands';
import { rememberWebTool } from '$lib/plugin-host/web.svelte';
import { projects, toasts, tools, ui } from '$lib/stores';

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
): Promise<true | false | ToolCheck> {
  try {
    const handle = await toolOpen({
      project_id: projectId,
      tool_id: tool.id,
      ctx: { ...EMPTY_CTX, ...ctx },
      placement: 'new_tab',
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
  const label = tools.list(projectId).find((t) => t.id === toolId)?.label ?? toolId;
  const r = await openTool(projectId, { id: toolId, label }, (args?.ctx ?? {}) as Partial<TemplateCtx>);
  // A missing binary opens the picker on that tool with its install hint and "Check again".
  if (typeof r === 'object')
    ui.openSheet('tool_picker', { projectId, query: toolId, checks: { [toolId]: r } });
});
