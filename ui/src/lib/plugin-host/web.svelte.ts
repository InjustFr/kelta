// Running web tools, keyed by tool instance id. Web panes only carry the instance id, so the URL
// and embed mode come from `tool_open` results (UI-opened tools) and from the backend's
// `kelta.tool_handle` / `kelta.tool_exited` relays (`plugin.event` with the tool instance id).

import type { EmbedMode, JsonValue, ProjectId, ToolHandle, ToolId, WebLifecycle } from '$lib/gen';
import { onUiEvent } from '$lib/ipc/events';

export interface WebTool {
  instanceId: string;
  toolId: ToolId;
  label: string;
  projectId: ProjectId;
  url: string;
  embed: EmbedMode;
  lifecycle: WebLifecycle;
  exited: { code: number; log: string } | null;
}

export const TOOL_HANDLE_EVENT = 'kelta.tool_handle';
export const TOOL_EXITED_EVENT = 'kelta.tool_exited';

export const webTools = $state<Record<string, WebTool>>({});

export function rememberWebTool(
  handle: ToolHandle,
  meta: { toolId: ToolId; label: string; projectId: ProjectId; lifecycle?: WebLifecycle },
): void {
  if (handle.kind !== 'web') return;
  webTools[handle.instance_id] = {
    instanceId: handle.instance_id,
    url: handle.url,
    embed: handle.embed,
    toolId: meta.toolId,
    label: meta.label,
    projectId: meta.projectId,
    lifecycle: meta.lifecycle ?? webTools[handle.instance_id]?.lifecycle ?? 'on_close',
    exited: null,
  };
}

export function forgetWebTool(instanceId: string): void {
  delete webTools[instanceId];
}

/** Applies a relayed `plugin.event`; returns false when it is not a web tool event. */
export function applyWebToolEvent(instanceId: string, name: string, payload: JsonValue): boolean {
  const p = (payload ?? {}) as Record<string, JsonValue>;
  if (name === TOOL_HANDLE_EVENT) {
    rememberWebTool(p.handle as unknown as ToolHandle, {
      toolId: String(p.tool_id),
      label: String(p.label ?? p.tool_id),
      projectId: String(p.project_id),
      lifecycle: (p.lifecycle as WebLifecycle | undefined) ?? 'on_close',
    });
    return true;
  }
  if (name === TOOL_EXITED_EVENT) {
    const t = webTools[instanceId];
    if (t) t.exited = { code: Number(p.code ?? -1), log: String(p.log ?? '') };
    return true;
  }
  return false;
}

onUiEvent('plugin.event', (ev) => void applyWebToolEvent(ev.instance_id, ev.name, ev.payload));
