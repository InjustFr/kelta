// First boot (#343): a fresh install opens the onboarding sheet, which offers "Set up with Claude"
// next to the manual steps.

import { projects, ui } from '$lib/stores';

/** Set when the onboarding sheet is closed. */
export const ONBOARDING_DONE = 'kelta.onboarding.done';

/** Opens the onboarding on a fresh install: no project of its own and the sheet never closed. */
export function offerOnboarding(): void {
  let done = false;
  try {
    done = localStorage.getItem(ONBOARDING_DONE) === '1';
  } catch {
    // storage can be unavailable: offered again, as the sheet says
  }
  if (!done && projects.loaded && projects.list.every((p) => p.builtin)) ui.openSheet('onboarding');
}
