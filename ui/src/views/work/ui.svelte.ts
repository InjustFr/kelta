// Work bar requests made from elsewhere (work menu, Now, palette): the work bar of that item's tab
// opens the menu.

import type { WorkItemId } from '$lib/gen';

export const workUi = $state<{ menu: WorkItemId | null }>({ menu: null });

/** `⌘.` (#404) sends this to the focused element; it bubbles until a pane opens its menu (cancelling it). */
export const ACTIONS_EVENT = 'kelta:actions';

/** `use:onActions={open}`: `⌘.` inside the element runs `open`, which opens the menu of its selection
 *  and returns true, or false when it has nothing selected (the event goes on up). */
export function onActions(el: HTMLElement, open: () => boolean): { destroy: () => void } {
  const on = (e: Event): void => {
    if (!open()) return;
    e.preventDefault();
    e.stopPropagation();
  };
  el.addEventListener(ACTIONS_EVENT, on);
  return { destroy: () => el.removeEventListener(ACTIONS_EVENT, on) };
}
