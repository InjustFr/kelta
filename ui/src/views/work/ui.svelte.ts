// Work bar requests made from elsewhere (work menu, Now, palette): the work bar of that item's tab
// opens the menu.

import type { WorkItemId } from '$lib/gen';

export const workUi = $state<{ menu: WorkItemId | null }>({ menu: null });

/** `⌘.` (#404) sends this to the focused element; it bubbles until a pane opens its menu (cancelling it). */
export const ACTIONS_EVENT = 'kelta:actions';

/** `use:onActions={open}`: `⌘.` inside the element runs `open`, which opens the menu of its selection
 *  and returns true, or false when it has nothing selected (the event goes on up). */
export function onActions(el: HTMLElement, open: () => boolean): { destroy: () => void } {
  return onActionsOf(ACTIONS_EVENT, el, open);
}

/** `⇧⌘R` (#417) sends this to the focused element: the list or detail around it refreshes. */
export const REFRESH_EVENT = 'kelta:refresh';

/** `use:onRefresh={refresh}`: `⇧⌘R` inside the element runs `refresh` (and goes no further up). */
export function onRefresh(el: HTMLElement, refresh: () => void): { destroy: () => void } {
  return onActionsOf(REFRESH_EVENT, el, () => (refresh(), true));
}

function onActionsOf(name: string, el: HTMLElement, open: () => boolean): { destroy: () => void } {
  const on = (e: Event): void => {
    if (!open()) return;
    e.preventDefault();
    e.stopPropagation();
  };
  el.addEventListener(name, on);
  return { destroy: () => el.removeEventListener(name, on) };
}

/** Sends `name` to the focused element; true when an element around it took it. */
export function askFocused(name: string): boolean {
  const ask = new Event(name, { bubbles: true, cancelable: true });
  document.activeElement?.dispatchEvent(ask);
  return ask.defaultPrevented;
}
