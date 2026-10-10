// A review's `linked_tickets` are bare keys: resolve one by exact key and open its detail.

import { trackerSearch } from '$lib/ipc/commands';
import type { ProjectId } from '$lib/gen';
import { toasts } from '$lib/stores';

import { activateProject } from '../../shell/nav';
import { openContent } from '../work/nav';

// `#12` badges: GitHub/GitLab/Gitea name tickets `repo#12`, Redmine uses the bare `12`.
export function sameKey(k: string, key: string, repo: string): boolean {
  k = k.toLowerCase();
  key = key.toLowerCase();
  return k === key || (key.startsWith('#') && (k === repo.toLowerCase() + key || '#' + k === key));
}

export async function openLinkedTicket(key: string, projectId: ProjectId, repo: string): Promise<void> {
  try {
    // The search is fuzzy (`SHOP-1` also finds `SHOP-12`): only an exact key counts.
    const hits = (await trackerSearch({ scope: { kind: 'all' }, text: key })).filter((h) =>
      sameKey(h.ticket.ref.key, key, repo),
    );
    const hit = hits.find((h) => h.project_ids.includes(projectId)) ?? hits[0];
    if (!hit) {
      toasts.info(`${key} is not in any of your trackers`);
      return;
    }
    // Land in the project on screen when the ticket belongs to it, else switch to its first project.
    const target = hit.project_ids.includes(projectId) ? projectId : (hit.project_ids[0] ?? projectId);
    await activateProject(target);
    await openContent(target, { kind: 'ticket_detail', ticket: hit.ticket.ref }, { placement: 'new_tab' });
  } catch (err) {
    toasts.error(err, `Opening ${key}`);
  }
}
