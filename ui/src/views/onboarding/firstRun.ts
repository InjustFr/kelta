// First boot (#343): a fresh install opens the onboarding sheet, which offers "Set up with Claude"
// next to the manual steps.

import { msg } from '$lib/i18n/index.svelte';
import type { ProjectInfo, WorkItem } from '$lib/gen';
import { projects, ui } from '$lib/stores';

import { CLAUDE, PR, TICKET } from '../work/stage';

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

/** Set once a first item has shipped (#422): Your loop never shows again. */
export const LOOP_SHIPPED = 'kelta.loop.shipped';

export type LoopStepId = 'project' | 'source' | 'start' | 'review' | 'ship';

/** The five steps of Your loop, with the stage of the track each one sits at. */
export const LOOP_STEPS: readonly { id: LoopStepId; label: string; at: number }[] = [
  { id: 'project', label: msg('Add a project'), at: TICKET },
  { id: 'source', label: msg('Connect a ticket source'), at: TICKET },
  { id: 'start', label: msg('Start work on a ticket'), at: CLAUDE },
  { id: 'review', label: msg('Review what Claude changed'), at: CLAUDE },
  { id: 'ship', label: msg('Ship'), at: PR },
];

/** What each step checks: real state, never a click counter. */
export function loopDone(
  list: readonly Pick<ProjectInfo, 'builtin'>[],
  hasSources: boolean | null,
  work: readonly Pick<WorkItem, 'pr_url' | 'claude_at' | 'review_due' | 'claude_replied'>[],
): Record<LoopStepId, boolean> {
  return {
    project: list.some((p) => !p.builtin),
    source: hasSources === true,
    start: work.length > 0,
    // Claude stopped on changes and the review was marked (or sent back): nothing is due any more.
    review: work.some((w) => !!w.claude_at && !w.review_due && !w.claude_replied),
    ship: work.some((w) => !!w.pr_url),
  };
}

/** The first step not done yet; `null` once all five are. */
export const loopCurrent = (done: Record<LoopStepId, boolean>): LoopStepId | null =>
  LOOP_STEPS.find((s) => !done[s.id])?.id ?? null;

/** Whether a first item shipped in an earlier launch. */
export function loopShipped(): boolean {
  try {
    return localStorage.getItem(LOOP_SHIPPED) === '1';
  } catch {
    return false; // storage can be unavailable: the panel may show again
  }
}

export function rememberShipped(): void {
  try {
    localStorage.setItem(LOOP_SHIPPED, '1');
  } catch {
    // see loopShipped
  }
}
