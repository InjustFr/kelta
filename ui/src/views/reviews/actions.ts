// Action handlers owned by L9 (reviews): `reviews.open`.
import { registerAction } from '$lib/actions';
import { projects } from '$lib/stores';

import { openView } from '../../shell/nav';
import { openContent } from '../work/nav';

// Without `project_id`: the cross-project Reviews view on the rail.
registerAction('reviews.open', async (args) => {
  const projectId = args?.project_id as string | undefined;
  if (!projectId) {
    openView('reviews');
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
