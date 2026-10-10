import type { ProjectId } from '$lib/gen';
import { sessionSpawnTemplate } from '$lib/ipc/commands';
import { sessions, toasts } from '$lib/stores';

/** One-click start of a built-in session template (`shell`, `claude+editor`) in a new tab. */
export async function spawnTemplate(projectId: ProjectId, templateId: string): Promise<void> {
  try {
    const created = await sessionSpawnTemplate({
      project_id: projectId,
      template_id: templateId,
      ctx: {
        repo_id: null,
        cwd: null,
        session_id: null,
        work_item_id: null,
        ticket: null,
        review: null,
        extra: {},
      },
      placement: 'new_tab',
    });
    for (const s of created) sessions.upsert(s);
  } catch (err) {
    toasts.error(err, 'Starting the session failed');
  }
}
