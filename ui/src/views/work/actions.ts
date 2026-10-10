// Work actions (FLOW §2.5): one registry for the work menu (⌘.), the work bar's primary button,
// Now's row keys and the palette's "Work: …" commands. Actions another lane builds (push, fix,
// rebase, link…) are listed with `run: null` and show disabled with "not available yet".
// Also owns `work.start`.

import { registerAction } from '$lib/actions';
import type { TicketRef, WorkItem } from '$lib/gen';
import { openExternal, workDiff, workMarkReviewed, workRetryStep } from '$lib/ipc/commands';
import { projects, sessions, tickets, toasts, ui, work } from '$lib/stores';

import { currentTab, revealSession } from '../../shell/nav';
import { claudeOf, phaseNow, prOf } from './live';
import { goToWork, openReview, showZoomedInWorkTab } from './nav';
import type { Phase, WorkActionId } from './phase';
import { selection } from './selection.svelte';
import { startWorkOnTicket } from './startWork';
import { workUi } from './ui.svelte';

export const NOT_YET = 'not available yet';
const READ_ONLY = 'Review checkout: read-only';

interface Ctx {
  item: WorkItem;
  phase: Phase;
}

export interface WorkAction {
  id: WorkActionId;
  /** Letter in the work menu and on Now rows (`F` = Shift+F). Null: primary-only. */
  key: string | null;
  label: (c: Ctx) => string;
  /** Why it cannot run now, or null. */
  blocked: (c: Ctx) => string | null;
  /** Null until the owning lane wires it. */
  run: ((c: Ctx) => Promise<void>) | null;
}

const review = (c: Ctx) => c.item.kind === 'review';
const stopped = (c: Ctx) => (c.phase.id === 'rebase_stopped' ? null : 'Only while a rebase is stopped');
const hasPr = (c: Ctx) => c.item.pr_url !== null || prOf(c.item) !== null;

async function step(c: Ctx, step: string): Promise<void> {
  work.upsert(await workRetryStep({ id: c.item.id, step }));
}

async function goClaude({ item }: Ctx): Promise<void> {
  const claude = claudeOf(item);
  // A live or dormant Claude is shown (attaching resumes a dormant one); a dead one is respawned.
  if (claude && claude.lifecycle !== 'exited' && (await revealSession(claude.id))) return;
  await goToWork(item);
}

async function reviewDiff({ item }: Ctx): Promise<void> {
  await goToWork(item);
  const s = sessions.upsert(await workDiff({ id: item.id }));
  showZoomedInWorkTab(item, s);
}

async function openTicket({ item }: Ctx): Promise<void> {
  if (!item.ticket) return;
  const d = await tickets.loadDetail(item.ticket);
  if (d.data) await openExternal({ url: d.data.ticket.url });
}

export const WORK_ACTIONS: readonly WorkAction[] = [
  {
    id: 'review_diff',
    key: 'd',
    label: () => 'Review diff in nvim',
    blocked: (c) => (c.phase.id === 'missing' ? 'Worktree missing' : null),
    run: reviewDiff,
  },
  {
    id: 'ship',
    key: 'p',
    label: (c) => (c.phase.id === 'rebased' ? 'Force push…' : hasPr(c) ? 'Push' : 'Ship'),
    blocked: (c) => (review(c) ? READ_ONLY : c.phase.id === 'rebased' || hasPr(c) ? NOT_YET : null),
    // shortcut: Ship opens today's Create PR dialog; the ship-finish lane renames it and adds Push.
    run: async ({ item }) => {
      await goToWork(item);
      workUi.ship = item.id;
    },
  },
  {
    id: 'mark_reviewed',
    key: 'x',
    label: () => 'Mark reviewed',
    blocked: (c) => (c.item.review_due ? null : 'Nothing to review'),
    run: async ({ item }) => void work.upsert(await workMarkReviewed({ id: item.id })),
  },
  {
    id: 'fix',
    key: 'f',
    label: () => 'Fix with Claude',
    blocked: (c) => (review(c) ? READ_ONLY : hasPr(c) ? NOT_YET : 'Needs a PR'),
    run: null,
  },
  {
    id: 'rebase',
    key: 'r',
    label: (c) => `Rebase onto ${c.item.base}`,
    blocked: (c) => (review(c) ? READ_ONLY : NOT_YET),
    run: null,
  },
  {
    id: 'rebase_continue',
    key: 'c',
    label: () => 'Continue rebase',
    blocked: (c) => stopped(c) ?? NOT_YET,
    run: null,
  },
  {
    id: 'rebase_abort',
    key: 'a',
    label: () => 'Abort rebase',
    blocked: (c) => stopped(c) ?? NOT_YET,
    run: null,
  },
  {
    id: 'conflicts',
    key: 'n',
    label: () => 'Open conflicts in nvim',
    blocked: (c) => stopped(c) ?? NOT_YET,
    run: null,
  },
  {
    id: 'skip_step',
    key: 's',
    label: (c) => (c.item.state.kind === 'failed' ? `Skip ${c.item.state.step}` : 'Skip step'),
    blocked: (c) => (c.item.state.kind === 'failed' ? null : 'Only when a start step failed'),
    run: (c) => step(c, `skip:${failedStep(c)}`),
  },
  { id: 'go_claude', key: 'g', label: () => 'Go to Claude', blocked: () => null, run: goClaude },
  {
    id: 'link',
    key: 'l',
    label: () => 'Link to ticket…',
    blocked: (c) => (review(c) ? READ_ONLY : c.item.kind !== 'branch' ? 'Already linked to a ticket' : null),
    run: async ({ item }) => ui.openSheet('link_ticket', { id: item.id }),
  },
  {
    id: 'open_ticket',
    key: 't',
    label: () => 'Open ticket',
    blocked: (c) => (c.item.ticket ? null : 'No ticket'),
    run: openTicket,
  },
  {
    id: 'open_pr',
    key: 'o',
    label: () => 'Open PR',
    blocked: (c) => (hasPr(c) ? null : 'No PR yet'),
    run: async ({ item }) => {
      const url = item.pr_url ?? prOf(item)?.url;
      if (url) await openExternal({ url });
    },
  },
  {
    id: 'finish',
    key: 'F',
    label: () => 'Finish…',
    blocked: () => null,
    run: async ({ item }) => {
      await goToWork(item);
      workUi.finish = item.id;
    },
  },
  // Primary-only actions (no letter of their own).
  {
    id: 'retry',
    key: null,
    label: (c) => c.phase.primaryLabel,
    blocked: () => null,
    run: (c) => step(c, failedStep(c)),
  },
  {
    id: 'recreate',
    key: null,
    label: () => 'Recreate',
    blocked: () => null,
    run: async (c) => {
      await step(c, 'worktree');
      await goToWork(c.item);
    },
  },
  { id: 'resolve', key: null, label: () => 'Ask Claude to resolve', blocked: () => NOT_YET, run: null },
  {
    id: 'open_review',
    key: null,
    label: () => 'Open review',
    blocked: (c) => (c.item.review ? null : 'No PR'),
    run: async ({ item }) => {
      if (item.review) await openReview(item.project_id, { kind: 'review_detail', review: item.review });
    },
  },
];

function failedStep(c: Ctx): string {
  return c.item.state.kind === 'failed' ? c.item.state.step : '';
}

export function workAction(id: WorkActionId): WorkAction {
  const a = WORK_ACTIONS.find((x) => x.id === id);
  if (!a) throw new Error(`unknown work action ${id}`);
  return a;
}

/** Why `id` cannot run on `item` now (null = it can). */
export function blockedReason(
  id: WorkActionId,
  item: WorkItem,
  phase: Phase = phaseNow(item),
): string | null {
  const a = workAction(id);
  return a.blocked({ item, phase }) ?? (a.run ? null : NOT_YET);
}

/** Runs a work action; a blocked one flashes its reason instead. */
export async function runWorkAction(id: WorkActionId, item: WorkItem): Promise<void> {
  const phase = phaseNow(item);
  const a = workAction(id);
  const reason = blockedReason(id, item, phase);
  if (reason || !a.run) {
    toasts.info(`${a.label({ item, phase })}: ${reason ?? NOT_YET}`);
    return;
  }
  try {
    await a.run({ item, phase });
  } catch (err) {
    toasts.error(err, a.label({ item, phase }));
  }
}

/** The phase's next action (`Enter` everywhere). */
export async function runPrimary(item: WorkItem): Promise<void> {
  const phase = phaseNow(item);
  if (phase.primary) await runWorkAction(phase.primary, item);
  else await goToWork(item);
}

/** Work item of the active project's focused tab. */
export function focusedWorkItem(): WorkItem | null {
  if (ui.inboxActive || !projects.activeId) return null;
  const id = currentTab()?.work_item_id;
  return id ? work.get(id) : null;
}

function onFocused(fn: (item: WorkItem) => void | Promise<void>): () => Promise<void> {
  return async () => {
    const item = focusedWorkItem();
    if (item) await fn(item);
    else toasts.info('Focus a work item tab first');
  };
}

registerAction(
  'work.menu',
  onFocused((item) => void (workUi.menu = item.id)),
);
registerAction('work.next', onFocused(runPrimary));
for (const a of WORK_ACTIONS)
  if (a.key)
    registerAction(
      `work.${a.id}`,
      onFocused((item) => runWorkAction(a.id, item)),
    );
registerAction('work.finish_merged', () => void toasts.info(`Finish all merged: ${NOT_YET}`));

registerAction('work.start', async (args) => {
  const ticket = (args?.ticket as TicketRef | undefined) ?? selection.ticket;
  if (!ticket) {
    toasts.info('Select a ticket to start work');
    return;
  }
  const projectId = (args?.project_id as string | undefined) ?? selection.projectId;
  await startWorkOnTicket(ticket, projectId);
});

// Preloaded: ⇧⌘N then typing at once must not lose the first keys to the focused terminal.
void import('./NewWorkSheet.svelte');

/** New work item sheet (FLOW §4.3): task, wip/ branch, Claude. */
export function newWorkItem(): void {
  ui.openSheet('work_new');
}

registerAction('work.new', newWorkItem);
