// Action handlers owned by L9 (reviews): `reviews.open`.
import { registerAction } from '$lib/actions';

import { openView } from '../../shell/nav';
import { listScope } from '../../shell/scope.svelte';
import { askFocused, REFRESH_EVENT } from '../work/ui.svelte';

// With `project_id`: the Reviews view scoped to that project (#409), no tab of its own.
// Its chord (`⇧⌘R`, the key manager passes `chord`) refreshes the list or detail that has the keys instead
// (#417); the leader key and the palette always open Reviews.
registerAction('reviews.open', (args) => {
  const projectId = args?.project_id as string | undefined;
  if (args?.chord && !projectId && askFocused(REFRESH_EVENT)) return;
  if (projectId) listScope.set(projectId);
  openView('reviews');
});
