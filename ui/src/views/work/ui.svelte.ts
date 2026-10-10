// Work bar requests made from elsewhere (work menu, Now, palette): the work bar of that item's tab
// opens the menu or the dialog.

import type { WorkItemId } from '$lib/gen';

export const workUi = $state<{ menu: WorkItemId | null; ship: WorkItemId | null; finish: WorkItemId | null }>(
  {
    menu: null,
    ship: null,
    finish: null,
  },
);
