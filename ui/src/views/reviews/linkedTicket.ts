// A review's `linked_tickets` are bare keys: resolve one by exact key and open its detail.

import { t } from '$lib/i18n/index.svelte';
import { trackerSearch } from '$lib/ipc/commands';
import type { ProjectId } from '$lib/gen';
import { projects, tickets, toasts, ui } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

import { activateProject, openView } from '../../shell/nav';
import { globalContent, openContent } from '../work/nav';

// `#12` badges: GitHub/GitLab/Gitea name tickets `repo#12`, Redmine uses the bare `12`.
export function sameKey(k: string, key: string, repo: string): boolean {
  k = k.toLowerCase();
  key = key.toLowerCase();
  return k === key || (key.startsWith('#') && (k === repo.toLowerCase() + key || '#' + k === key));
}

export async function openLinkedTicket(
  key: string,
  projectId: ProjectId | null,
  repo: string,
): Promise<void> {
  // From a global view, a ticket the Tickets view has loaded lands there instead of opening a project.
  const tv = ui.view !== null ? globalContent('tickets') : null;
  const loaded =
    tv?.kind === 'tickets' &&
    tickets
      .items(tv.scope, tv.view_id, tv.who, tv.source_id ?? null)
      .find((i) => sameKey(i.ticket.ref.key, key, repo));
  if (loaded) {
    openView('tickets', ticketKey(loaded.ticket.ref));
    return;
  }
  try {
    // The search is fuzzy (`SHOP-1` also finds `SHOP-12`): only an exact key counts.
    const hits = (await trackerSearch({ scope: { kind: 'all' }, text: key })).filter((h) =>
      sameKey(h.ticket.ref.key, key, repo),
    );
    const hit = (projectId && hits.find((h) => h.project_ids.includes(projectId))) || hits[0];
    if (!hit) {
      toasts.info(t('{key} is not in any of your trackers', { key }));
      return;
    }
    // Land in the project on screen when the ticket belongs to it, else switch to its first project
    // (from a global view: else the active one).
    const target =
      projectId && hit.project_ids.includes(projectId)
        ? projectId
        : (hit.project_ids[0] ?? projectId ?? projects.activeId);
    if (!target) return;
    await activateProject(target);
    await openContent(target, { kind: 'ticket_detail', ticket: hit.ticket.ref }, { placement: 'new_tab' });
  } catch (err) {
    toasts.error(err, t('Opening {key}', { key }));
  }
}
