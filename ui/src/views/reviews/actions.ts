// Action handlers owned by L9 (reviews): `reviews.open`.
import { registerAction } from '$lib/actions';
import { projects, toasts } from '$lib/stores';

import { openContent } from '../work/nav';

registerAction('reviews.open', async (args) => {
  const projectId = (args?.project_id as string | undefined) ?? projects.activeId;
  if (!projectId) {
    toasts.info('Open a project first');
    return;
  }
  const home = projects.byId(projectId)?.builtin ?? false;
  const scope = home ? ({ kind: 'all' } as const) : ({ kind: 'project', id: projectId } as const);
  await openContent(
    projectId,
    { kind: 'reviews', scope },
    {
      placement: 'new_tab',
      title: 'Reviews',
      match: (c) =>
        c.kind === 'reviews' &&
        c.scope.kind === scope.kind &&
        (c.scope.kind === 'all' || c.scope.id === projectId),
    },
  );
});
