// Action handlers owned by L9 (reviews): `reviews.open`.
import { registerAction } from '$lib/actions';

import { openView } from '../../shell/nav';
import { listScope } from '../../shell/scope.svelte';

// With `project_id`: the Reviews view scoped to that project (#409), no tab of its own.
registerAction('reviews.open', (args) => {
  const projectId = args?.project_id as string | undefined;
  if (projectId) listScope.set(projectId);
  openView('reviews');
});
