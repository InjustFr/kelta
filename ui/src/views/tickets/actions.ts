// Action handlers owned by L9 (tickets): `tickets.open` (+ the toast helper `tickets.open_in_browser`).
import { registerAction } from '$lib/actions';
import { openExternal } from '$lib/ipc/commands';
import { projects, toasts, ui } from '$lib/stores';

import { openContent } from '../work/nav';

registerAction('tickets.open', async (args) => {
  const projectId = (args?.project_id as string | undefined) ?? projects.activeId;
  if (!projectId) {
    toasts.info('Open a project first');
    return;
  }
  await openContent(
    projectId,
    { kind: 'tickets', scope: { kind: 'project', id: projectId }, view_id: null, mode: 'list', who: null },
    {
      placement: 'new_tab',
      title: 'Tickets',
      match: (c) => c.kind === 'tickets' && c.scope.kind === 'project' && c.scope.id === projectId,
    },
  );
});

registerAction('tickets.groom', () => ui.openSheet('groom'));

registerAction('tickets.open_in_browser', async (args) => {
  const url = args?.url;
  if (typeof url !== 'string') return;
  try {
    await openExternal({ url });
  } catch (err) {
    toasts.error(err, 'Open in browser');
  }
});
