// Refine with Claude (TICKETS.md T10): one proposal per ticket, kept until discarded. Starting work
// on a ticket with a kept proposal adds its acceptance criteria to Claude's first prompt.
// shortcut: proposals live in memory only (lost on reload); persist them if refines must outlive the window.

import type { ProjectId, StartWorkPlan, TicketRef } from '$lib/gen';
import { trackerRefine } from '$lib/ipc/commands';
import { toasts } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

export interface Refine {
  running: boolean;
  /** Claude's Markdown proposal; null while the first run is going. */
  text: string | null;
  posted: boolean;
}

export const refines = $state<Record<string, Refine>>({});

export async function refine(ref: TicketRef, projectId: ProjectId | null): Promise<void> {
  const key = ticketKey(ref);
  const prev = refines[key];
  if (prev?.running) return;
  refines[key] = { running: true, text: prev?.text ?? null, posted: false };
  try {
    const text = await trackerRefine({ ticket: ref, project_id: projectId });
    refines[key] = { running: false, text, posted: false };
  } catch (err) {
    if (prev) refines[key] = prev;
    else delete refines[key];
    toasts.error(err, `Refining ${ref.key}`);
  }
}

export function discard(ref: TicketRef): void {
  delete refines[ticketKey(ref)];
}

/** The body of the `## Acceptance criteria` section of `md` (null when missing or empty). */
export function acceptanceCriteria(md: string): string | null {
  const lines = md.split('\n');
  const start = lines.findIndex((l) => /^#{1,6}\s+acceptance criteria\s*$/i.test(l.trim()));
  if (start < 0) return null;
  const end = lines.findIndex((l, i) => i > start && /^#{1,6}\s/.test(l.trim()));
  const body = lines
    .slice(start + 1, end < 0 ? undefined : end)
    .join('\n')
    .trim();
  return body === '' ? null : body;
}

/** The accepted criteria of `ref`'s kept proposal, for the start-work prompt. */
export function criteriaFor(ref: TicketRef): string | null {
  const text = refines[ticketKey(ref)]?.text;
  return text ? acceptanceCriteria(text) : null;
}

/** Appends `ref`'s accepted criteria to the plan's first prompt (a template rendered again at spawn). */
export function withCriteria(plan: StartWorkPlan, ref: TicketRef): StartWorkPlan {
  const criteria = criteriaFor(ref);
  // Braces escaped: `{x}` in the prompt is a placeholder, `{{`/`}}` collapse back to literals.
  if (criteria)
    plan.claude.prompt += `\n\nAcceptance criteria (from the refine):\n${criteria.replaceAll('{', '{{').replaceAll('}', '}}')}`;
  return plan;
}
