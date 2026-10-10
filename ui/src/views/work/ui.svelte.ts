// Work bar requests made from elsewhere (work menu, Now, palette): the work bar of that item's tab
// opens the menu.

import type { WorkItemId } from '$lib/gen';

export const workUi = $state<{ menu: WorkItemId | null }>({ menu: null });
