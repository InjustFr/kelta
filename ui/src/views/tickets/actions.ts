// Action handlers owned by L9 (tickets): `tickets.open`, `tickets.board`, `tickets.overview` (+ the
// toast helper `tickets.open_in_browser`).
import { t } from '$lib/i18n/index.svelte';
import { registerAction } from '$lib/actions';
import type { ProjectId, Scope, TicketsMode } from '$lib/gen';
import { openExternal } from '$lib/ipc/commands';
import { activateTab, addTab, findTab, makeTab, pane, splitPane } from '$lib/layout';
import { GLOBAL, layout, projects, toasts, ui } from '$lib/stores';

import { activateProject, openView } from '../../shell/nav';
import { listScope } from '../../shell/scope.svelte';
import { loadGlobalLayout, persistPaneContent } from '../work/nav';

/** A project's tickets: the Tickets view scoped to it (#409), no tab of their own; `mode` = as it was. */
async function openProjectTickets(projectId: ProjectId, mode?: TicketsMode): Promise<void> {
  listScope.set(projectId);
  if (mode) {
    await loadGlobalLayout().catch(() => null);
    persistPaneContent(GLOBAL, 'tickets', { mode });
  }
  openView('tickets');
}

// Without `project_id`: the Tickets view in its current scope.
registerAction('tickets.open', async (args) => {
  const projectId = args?.project_id as string | undefined;
  if (projectId) await openProjectTickets(projectId);
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

/** `tickets.overview`'s tab id: one per project, focused when open, rebuilt once closed. */
const OVERVIEW_TAB = 'overview';

// My tickets beside Reviews (its sections: asked of me, team, mine), in the active project.
registerAction('tickets.overview', async () => {
  const projectId = projects.activeId;
  if (!projectId) {
    toasts.info(t('Open a project first'));
    return;
  }
  await activateProject(projectId); // leaves a global view
  try {
    await layout.ensure(projectId);
  } catch {
    return;
  }
  const scope: Scope = { kind: 'project', id: projectId };
  // The builtin home project binds no repo: its reviews are every project's (as `reviews.open`).
  const reviewScope: Scope = projects.byId(projectId)?.builtin ? { kind: 'all' } : scope;
  layout.update(projectId, (l) => {
    if (findTab(l, OVERVIEW_TAB)) return activateTab(l, OVERVIEW_TAB);
    const tickets = pane({ kind: 'tickets', scope, view_id: null, mode: 'list', who: 'mine' });
    const root = splitPane(tickets, tickets.id, 'row', pane({ kind: 'reviews', scope: reviewScope }));
    return addTab(l, makeTab('Overview', root, { id: OVERVIEW_TAB }));
  });
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
