// Work actions (FLOW §2.5): one registry for the work menu (⌘.), the work bar's primary button,
// Now's row keys and the palette's "Work: …" commands. Actions another lane builds (push, fix,
// rebase, link…) are listed with `run: null` and show disabled with "not available yet".
// Also owns `work.start`.

import { msg, t } from '$lib/i18n/index.svelte';
import { registerAction } from '$lib/actions';
import type { TicketRef, WorkItem } from '$lib/gen';
import {
  openExternal,
  workDiff,
  workDisarmMerge,
  workMarkReviewed,
  workPark,
  workQueueFront,
  workRetryStep,
  workSetNote,
  workStartNow,
} from '$lib/ipc/commands';
import { projects, sessions, tickets, toasts, ui, work } from '$lib/stores';

import { prompts } from '../../shell/confirm.svelte';
import { currentTab, revealSession } from '../../shell/nav';
import {
  askClaudeToResolve,
  fixChecks,
  fixItem,
  forcePushItem,
  openConflicts,
  pushItem,
  rebase,
  rebaseStep,
} from './fixloop.svelte';
import { workActionDisabled } from './common';
import { claudeOf, phaseNow, prOf } from './live';
import { goToWork, openReview, showZoomedInWorkTab } from './nav';
import type { Phase, WorkActionId } from './phase';
import { selection } from './selection.svelte';
import { startWorkOnTicket } from './startWork';
import { workUi } from './ui.svelte';

export const NOT_YET = msg('not available yet');

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

// A rebase is stopped while git has it open, also after Claude staged the last conflict.
const stopped = (c: Ctx) =>
  c.item.rebase && c.item.rebase.total > 0 ? null : t('Only while a rebase is stopped');
const queuedOnly = (c: Ctx) => (c.item.state.kind === 'queued' ? null : t('Not queued'));
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

async function reviewDiff(
  { item }: Ctx,
  args: { delta?: boolean; from?: string | null } = {},
): Promise<void> {
  await goToWork(item);
  const s = sessions.upsert(await workDiff({ id: item.id, ...args }));
  showZoomedInWorkTab(item, s);
}

/** `v`: own items since the last review; a review item since the PR head I reviewed. */
const reviewDelta = (c: Ctx): Promise<void> =>
  reviewDiff(c, c.item.kind === 'review' ? { from: prOf(c.item)?.reviewed_head ?? null } : { delta: true });

async function editNote({ item }: Ctx): Promise<void> {
  const note = await prompts.ask({
    title: t('Next step'),
    label: t('next:'),
    value: item.next_note ?? '',
    confirmLabel: t('Save'),
  });
  if (note !== null) work.upsert(await workSetNote({ id: item.id, note }));
}

async function openTicket({ item }: Ctx): Promise<void> {
  if (!item.ticket) return;
  const d = await tickets.loadDetail(item.ticket);
  if (d.data) await openExternal({ url: d.data.ticket.url });
}

export const WORK_ACTIONS: readonly WorkAction[] = [
  {
    id: 'review_delta',
    key: 'v',
    label: () => t('Review changes since last look'),
    blocked: (c) =>
      c.phase.id === 'missing'
        ? t('Worktree missing')
        : c.item.kind === 'review' && !prOf(c.item)?.reviewed_head
          ? t('Not reviewed yet')
          : null,
    run: reviewDelta,
  },
  {
    id: 'review_diff',
    key: 'V',
    label: () => t('Review the whole diff in nvim'),
    blocked: (c) => (c.phase.id === 'missing' ? t('Worktree missing') : null),
    run: (c) => reviewDiff(c),
  },
  {
    id: 'ship',
    key: 'p',
    label: (c) => (c.phase.id === 'rebased' ? t('Force push…') : hasPr(c) ? t('Push') : t('Ship')),
    blocked: () => null,
    run: async ({ item, phase }) => {
      if (phase.id === 'rebased') return forcePushItem(item);
      if (item.pr_url !== null) return pushItem(item);
      await goToWork(item);
      ui.openSheet('ship', { item });
    },
  },
  {
    id: 'mark_reviewed',
    key: 'R',
    label: () => t('Mark reviewed'),
    blocked: (c) => (c.item.review_due ? null : t('Nothing to review')),
    run: async ({ item }) => void work.upsert(await workMarkReviewed({ id: item.id })),
  },
  {
    id: 'fix',
    key: 'f',
    label: () => t('Fix with Claude'),
    blocked: (c) => (hasPr(c) ? null : t('Needs a PR')),
    run: async ({ item }) => fixItem(item),
  },
  {
    id: 'rebase',
    key: 'r',
    label: (c) => t('Rebase onto {base}', { base: c.item.base }),
    blocked: () => null,
    run: async ({ item, phase }) => rebase(item, phase.id === 'remote_new' ? 'remote_branch' : 'base'),
  },
  {
    id: 'rebase_continue',
    key: 'c',
    label: () => t('Continue rebase'),
    blocked: stopped,
    run: ({ item }) => rebaseStep(item, 'continue'),
  },
  {
    id: 'rebase_abort',
    key: 'a',
    label: () => t('Abort rebase'),
    blocked: stopped,
    run: ({ item }) => rebaseStep(item, 'abort'),
  },
  {
    id: 'conflicts',
    key: 'n',
    label: () => t('Open conflicts in nvim'),
    blocked: (c) => (c.item.rebase?.conflicts.length ? null : t('No conflicted files')),
    run: ({ item }) => openConflicts(item),
  },
  {
    id: 'skip_step',
    key: 's',
    label: (c) =>
      c.item.state.kind === 'failed' ? t('Skip {step}', { step: c.item.state.step }) : t('Skip step'),
    blocked: (c) => (c.item.state.kind === 'failed' ? null : t('Only when a start step failed')),
    run: (c) => step(c, `skip:${failedStep(c)}`),
  },
  {
    id: 'edit_note',
    key: 'b',
    label: (c) => (c.item.next_note ? t('Edit next: note…') : t('Add next: note…')),
    blocked: () => null,
    run: editNote,
  },
  { id: 'go_claude', key: 'g', label: () => t('Go to Claude'), blocked: () => null, run: goClaude },
  {
    id: 'link',
    key: 'l',
    label: () => t('Link to ticket…'),
    blocked: (c) => (c.item.kind !== 'branch' ? t('Already linked to a ticket') : null),
    run: async ({ item }) => ui.openSheet('link_ticket', { id: item.id }),
  },
  {
    id: 'create_ticket',
    key: 'T',
    label: () => t('Create ticket…'),
    blocked: (c) => (c.item.kind !== 'branch' ? t('Already linked to a ticket') : null),
    run: async ({ item }) => ui.openSheet('create_ticket', { id: item.id }),
  },
  {
    id: 'open_ticket',
    key: 't',
    label: () => t('Open ticket'),
    blocked: (c) => (c.item.ticket ? null : t('No ticket')),
    run: openTicket,
  },
  {
    id: 'open_pr',
    key: 'o',
    label: () => t('Open PR'),
    blocked: (c) => (hasPr(c) ? null : t('No PR yet')),
    run: async ({ item }) => {
      const url = item.pr_url ?? prOf(item)?.url;
      if (url) await openExternal({ url });
    },
  },
  {
    id: 'merge',
    key: 'M',
    label: (c) => (c.item.auto_finish ? t('Disarm merge when ready') : t('Merge when ready…')),
    blocked: (c) => (c.item.state.kind === 'pr_open' ? null : t('Needs an open PR')),
    run: async ({ item }) => {
      if (item.auto_finish) work.upsert(await workDisarmMerge({ id: item.id }));
      else ui.openSheet('merge', { item });
    },
  },
  {
    id: 'park',
    key: 'P',
    label: () => t('Park'),
    blocked: (c) => {
      const claude = claudeOf(c.item);
      if (claude?.status === 'working') return t('Claude is working');
      const live = c.item.session_ids.some((id) => {
        const s = sessions.get(id);
        return s?.lifecycle === 'live' && (s.kind.type === 'claude' || s.kind.type === 'editor');
      });
      return live ? null : t('Nothing running to park');
    },
    run: async ({ item }) => void work.upsert(await workPark({ id: item.id })),
  },
  {
    id: 'finish',
    key: 'F',
    label: () => t('Finish…'),
    blocked: () => null,
    run: async ({ item }) => {
      await goToWork(item);
      ui.openSheet('finish', { item });
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
    id: 'fix_checks',
    key: null,
    label: (c) => c.phase.primaryLabel,
    blocked: (c) => (hasPr(c) ? null : t('Needs a PR')),
    run: async ({ item }) => fixChecks(item),
  },
  {
    id: 'start_now',
    key: null,
    label: () => t('Start now (over cap)'),
    blocked: queuedOnly,
    run: async ({ item }) => void work.upsert(await workStartNow({ id: item.id })),
  },
  {
    id: 'queue_front',
    key: null,
    label: () => t('Move to front'),
    blocked: queuedOnly,
    run: async ({ item }) => void work.upsert(await workQueueFront({ id: item.id })),
  },
  {
    id: 'recreate',
    key: null,
    label: () => t('Recreate'),
    blocked: () => null,
    run: async (c) => {
      await step(c, 'worktree');
      await goToWork(c.item);
    },
  },
  {
    id: 'resolve',
    key: null,
    label: () => t('Ask Claude to resolve'),
    blocked: stopped,
    run: ({ item }) => askClaudeToResolve(item),
  },
  {
    id: 'open_review',
    key: null,
    label: () => t('Open review'),
    blocked: (c) => (c.item.review ? null : t('No PR')),
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
  // Review items: p/r/f/l are read-only (#131), whatever the action's own check says.
  return workActionDisabled(item, a.key ?? '') ?? a.blocked({ item, phase }) ?? (a.run ? null : t(NOT_YET));
}

/** Runs a work action; a blocked one flashes its reason instead. */
export async function runWorkAction(id: WorkActionId, item: WorkItem): Promise<void> {
  const phase = phaseNow(item);
  const a = workAction(id);
  const reason = blockedReason(id, item, phase);
  if (reason || !a.run) {
    toasts.info(`${a.label({ item, phase })}: ${reason ?? t(NOT_YET)}`);
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
  if (ui.view || !projects.activeId) return null;
  const id = currentTab()?.work_item_id;
  return id ? work.get(id) : null;
}

/** `args.id` names the item (palette, Now rows); default: the focused tab's. */
function onFocused(
  fn: (item: WorkItem) => void | Promise<void>,
): (args?: Record<string, unknown>) => Promise<void> {
  return async (args) => {
    const id = args?.id as string | undefined;
    const item = (id ? work.get(id) : null) ?? focusedWorkItem();
    if (item) await fn(item);
    else toasts.info(t('Focus a work item tab first'));
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
registerAction('work.finish_merged', () => ui.openSheet('finish_merged'));

registerAction('work.start', async (args) => {
  const ticket = (args?.ticket as TicketRef | undefined) ?? selection.ticket;
  if (!ticket) {
    toasts.info(t('Select a ticket to start work'));
    return;
  }
  // Only an explicit pick (a trigger's project_id): the backend picks the default (SPEC §3.1).
  await startWorkOnTicket(ticket, args?.project_id as string | undefined);
});

// Preloaded: ⇧⌘N then typing at once must not lose the first keys to the focused terminal.
void import('./NewWorkSheet.svelte');

/** New work item sheet (FLOW §4.3): task, wip/ branch, Claude. */
export function newWorkItem(): void {
  ui.openSheet('work_new');
}

registerAction('work.new', newWorkItem);
