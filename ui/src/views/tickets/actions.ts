// Action handlers owned by L9 (tickets): `tickets.open`, `tickets.board` (+ the toast helper
// `tickets.open_in_browser`).
import { t } from '$lib/i18n/index.svelte';
import { registerAction } from '$lib/actions';
import type { ProjectId, TicketsMode } from '$lib/gen';
import { openExternal } from '$lib/ipc/commands';
import { projects, toasts, ui } from '$lib/stores';

import { openView } from '../../shell/nav';
import { openContent } from '../work/nav';

/** A project's Tickets tab (the board needs one project's columns). */
async function openProjectTickets(projectId: ProjectId, mode: TicketsMode): Promise<void> {
  await openContent(
    projectId,
    { kind: 'tickets', scope: { kind: 'project', id: projectId }, view_id: null, mode, who: null },
    {
      placement: 'new_tab',
      title: 'Tickets',
      match: (c) => c.kind === 'tickets' && c.scope.kind === 'project' && c.scope.id === projectId,
      replace: mode === 'board',
    },
  );
}

// Without `project_id`: the cross-project Tickets view on the rail.
registerAction('tickets.open', async (args) => {
  const projectId = args?.project_id as string | undefined;
  if (projectId) await openProjectTickets(projectId, 'list');
  else openView('tickets');
});

registerAction('tickets.board', async () => {
  const projectId = projects.activeId;
  if (!projectId) {
    toasts.info(t('Open a project first'));
    return;
  }
  await openProjectTickets(projectId, 'board');
});

registerAction('tickets.groom', () => ui.openSheet('groom'));

registerAction('tickets.open_in_browser', async (args) => {
  const url = args?.url;
  if (typeof url !== 'string') return;
  try {
    await openExternal({ url });
  } catch (err) {
    toasts.error(err, t('Open in browser'));
  }
});
