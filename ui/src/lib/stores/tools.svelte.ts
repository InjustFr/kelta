// Tools per project (tool_list). Reloaded on settings.changed (tools come from config + plugins).

import type { ProjectId, ToolInfo, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { idle, settle, type Loadable } from './loadable';

export class ToolsStore {
  byProject = $state<Record<ProjectId, Loadable<ToolInfo[]>>>({});

  list(projectId: ProjectId): ToolInfo[] {
    return this.byProject[projectId]?.data ?? [];
  }

  async load(projectId: ProjectId): Promise<Loadable<ToolInfo[]>> {
    const prev = this.byProject[projectId] ?? idle<ToolInfo[]>();
    this.byProject = { ...this.byProject, [projectId]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.toolList({ project_id: projectId }));
    this.byProject = { ...this.byProject, [projectId]: next };
    return next;
  }

  apply(ev: UiEvent): void {
    if (ev.type === 'settings.changed') {
      for (const id of Object.keys(this.byProject)) void this.load(id);
    } else if (ev.type === 'project.removed' && ev.id in this.byProject) {
      const next = { ...this.byProject };
      delete next[ev.id];
      this.byProject = next;
    }
  }
}
