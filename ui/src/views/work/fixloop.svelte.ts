// Feedback loop, rebase and push from the UI (FLOW §4.2, §4.4, §4.5): the `work.*` action handlers,
// their refusal / confirmation dialogs and toasts, and the `feedback.md` / `conflicts.md` briefs.
// Every handler takes `{ id }` and falls back to the focused tab's work item (palette).

import { registerAction, type ActionArgs } from '$lib/actions';
import type { Feedback, ProjectId, ReviewRef, Toast, WorkItem, WorkItemId } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { isIpcError } from '$lib/ipc/transport';
import { activeTab } from '$lib/layout';
import { layout, projects, settings, toasts, ui, work } from '$lib/stores';

/** Bumped after an operation that changes git state: work headers re-read `work_status`. */
export const gitTick = $state({ n: 0 });

/** Reviewers of the last feedback read per item (label of "Re-request review from …"). */
// eslint-disable-next-line svelte/prefer-svelte-reactivity -- read once when a toast is built
const reviewersOf = new Map<WorkItemId, string[]>();

export const COMMIT_PROMPT =
  'Commit the uncommitted changes in this worktree with a clear message. Do not push.';

export interface DialogAction {
  label: string;
  /** Action id run through the registry; omitted = just close. */
  command?: string;
  args?: Record<string, unknown>;
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
}

export interface WorkDialogProps {
  title: string;
  text: string;
  tone?: 'default' | 'danger';
  /** Last one is the primary (`⌘↵`). */
  actions: DialogAction[];
}

const FALLBACK_PROMPTS = {
  feedback: 'Address the review feedback in {file}. Do not push.',
  conflicts: 'Resolve the rebase conflicts listed in {file}. Do not push.',
};

/** `claude.prompt_templates.<key>` (rendered by `work_send` with `{file}`, `{onto}`, …). */
export function promptTemplate(key: keyof typeof FALLBACK_PROMPTS): string {
  return settings.value()?.claude.prompt_templates[key] ?? FALLBACK_PROMPTS[key];
}

export function openWorkDialog(p: WorkDialogProps): void {
  ui.openSheet('work_dialog', { ...p });
}

export function focusedWorkItem(): WorkItemId | null {
  const pid = projects.activeId;
  const l = pid ? layout.get(pid) : null;
  return (l && activeTab(l)?.work_item_id) ?? null;
}

function target(args: ActionArgs): WorkItem | null {
  const id = (args?.id as WorkItemId | undefined) ?? focusedWorkItem();
  const w = id ? work.get(id) : null;
  if (!w) toasts.info('Focus a work item first');
  return w;
}

export function remoteOf(w: WorkItem): string {
  return projects.byId(w.project_id)?.repos.find((r) => r.id === w.repo_id)?.remote ?? 'origin';
}

export function prLabel(w: WorkItem): string {
  const n = w.pr_url?.match(/(\d+)\/?$/)?.[1];
  return n ? `#${n}` : 'the PR';
}

export function reasonOf(err: unknown): string {
  const d = isIpcError(err) ? err.detail : null;
  return d && typeof d === 'object' && !Array.isArray(d) && typeof d.reason === 'string' ? d.reason : '';
}

function done(w: WorkItem): WorkItem {
  work.upsert(w);
  gitTick.n++;
  return w;
}

function toast(level: Toast['level'], text: string, action: Toast['action'] = null): void {
  toasts.push({ level, text, action });
}

/** `Claude is working…` style refusals are warnings; anything else is an error toast. */
function refused(err: unknown, context: string): void {
  if (isIpcError(err, 'conflict')) toast('warn', err.message);
  else toasts.error(err, context);
}

// ---- briefs ----------------------------------------------------------------------------------

export interface FeedbackEntry {
  key: string;
  kind: 'thread' | 'review' | 'check';
  /** Host thread id (threads only): remembered to resolve them later. */
  threadId: string | null;
  title: string;
  /** What the sheet shows (comments, summary, log tail). */
  text: string;
  /** What feedback.md gets under the title. */
  md: string;
}

export function feedbackEntries(fb: Feedback): FeedbackEntry[] {
  return [
    ...fb.threads.map((t) => ({
      key: `t:${t.id}`,
      kind: 'thread' as const,
      threadId: t.id,
      title: `${t.author}${t.path ? ` on ${t.path}${t.line ? `:${t.line}` : ''}` : ''}`,
      text: t.body_md.trim(),
      md: `${t.body_md.trim()}\n\n<${t.url}>`,
    })),
    ...fb.reviews.map((r, i) => ({
      key: `r:${i}`,
      kind: 'review' as const,
      threadId: null,
      title: `${r.author}${r.state === 'changes_requested' ? ' (changes requested)' : r.state === 'approved' ? ' (approved)' : ''}`,
      text: r.body_md.trim(),
      md: r.body_md.trim(),
    })),
    ...fb.failed_checks.map((c) => ({
      key: `c:${c.name}`,
      kind: 'check' as const,
      threadId: null,
      title: c.name,
      text: c.log_tail?.trimEnd() ?? 'No log available.',
      md: [c.url ? `<${c.url}>` : '', c.log_tail ? `\`\`\`text\n${c.log_tail.trimEnd()}\n\`\`\`` : '']
        .filter(Boolean)
        .join('\n\n'),
    })),
  ];
}

const SECTIONS: Record<FeedbackEntry['kind'], string> = {
  thread: 'Unresolved threads',
  review: 'Reviews',
  check: 'Failed checks',
};

/** Same layout as `Feedback::to_markdown` (MCP), restricted to the checked entries. */
export function feedbackMarkdown(entries: FeedbackEntry[]): string {
  let s = '# Review feedback\n';
  for (const kind of ['thread', 'review', 'check'] as const) {
    const part = entries.filter((e) => e.kind === kind);
    if (part.length === 0) continue;
    s += `\n## ${SECTIONS[kind]}\n`;
    for (const e of part) s += `\n### ${e.title}\n${e.md ? `\n${e.md}\n` : ''}`;
  }
  return s;
}

export function conflictsMarkdown(w: WorkItem): string {
  const r = w.rebase;
  if (!r) return '';
  const files = r.conflicts.map((f) => `- \`${f}\``).join('\n');
  return `# Rebase conflicts\n\nRebasing \`${w.branch}\` onto \`${r.onto}\` stopped at commit ${r.step} of ${r.total}.\n\n## Conflicted files\n\n${files || '- (none left: continue the rebase)'}\n`;
}

// ---- operations ------------------------------------------------------------------------------

export function rememberReviewers(id: WorkItemId, fb: Feedback): void {
  reviewersOf.set(id, fb.reviewers);
}

async function rebase(w: WorkItem, onto: 'base' | 'remote_branch', noFetch = false): Promise<void> {
  try {
    const out = done(await ipc.workRebase({ id: w.id, op: { kind: 'start', onto, no_fetch: noFetch } }));
    const r = out.rebase;
    if (r && r.total > 0) {
      const n = r.conflicts.length;
      toast('warn', `Rebase stopped: ${n} conflicted file${n === 1 ? '' : 's'}.`, {
        label: 'Ask Claude to resolve',
        command: 'work.rebase_ask_claude',
        args: { id: w.id },
      });
    } else if (r) {
      toast('info', `Rebased onto ${r.onto}. Force push to update ${prLabel(w)}.`, {
        label: 'Force push…',
        command: 'work.force_push',
        args: { id: w.id },
      });
    } else if (onto === 'remote_branch') {
      // FLOW §4.4: the remote's commits are in, now the normal rebase onto base follows.
      // shortcut: a remote rebase stopped on conflicts does not chain after Continue; the item then shows Behind.
      await rebase(w, 'base', noFetch);
    } else {
      toast('info', `Rebased ${w.branch}.`);
    }
  } catch (err) {
    const reason = reasonOf(err);
    if (isIpcError(err, 'dirty')) {
      openWorkDialog({
        title: 'Rebase refused',
        text: err.message,
        actions: [
          { label: 'Cancel', variant: 'ghost' },
          { label: 'Open shell', command: 'pane.split_down' },
          {
            label: 'Ask Claude to commit',
            command: 'work.ask_commit',
            args: { id: w.id },
            variant: 'primary',
          },
        ],
      });
    } else if (reason === 'fetch_failed' && isIpcError(err)) {
      const detail = err.detail as { error?: string } | null;
      openWorkDialog({
        title: 'Fetch failed',
        text: `${err.message}${detail?.error ? ` ${detail.error}` : ''}`,
        actions: [
          { label: 'Cancel', variant: 'ghost' },
          {
            label: `Rebase onto last fetched ${w.base}`,
            command: 'work.rebase',
            args: { id: w.id, onto, no_fetch: true },
            variant: 'primary',
          },
        ],
      });
    } else {
      refused(err, 'Rebase');
    }
  }
}

async function push(w: WorkItem, force: boolean): Promise<void> {
  try {
    done(await ipc.workPush({ id: w.id, force }));
    const who = reviewersOf.get(w.id) ?? [];
    toast(
      'info',
      `${force ? 'Force pushed' : 'Pushed'} ${w.branch}.`,
      w.pr_url
        ? {
            label: who.length ? `Re-request review from ${who.join(', ')}` : 'Re-request review',
            command: 'work.rerequest_review',
            args: { id: w.id },
          }
        : null,
    );
  } catch (err) {
    const reason = reasonOf(err);
    if (reason === 'non_fast_forward' && isIpcError(err)) {
      toast('warn', err.message, {
        label: 'Rebase',
        command: 'work.rebase',
        args: { id: w.id, onto: 'remote_branch' },
      });
    } else if (reason === 'lease_rejected' && isIpcError(err)) {
      toast('error', err.message, { label: 'Fetch and show', command: 'work.show', args: { id: w.id } });
    } else {
      refused(err, force ? 'Force push' : 'Push');
    }
  }
}

/** Sends a prompt to the item's Claude; refusals (busy, hooks inactive) become toasts. */
export async function sendToClaude(
  w: WorkItem,
  prompt: string,
  files: { name: string; content: string }[] = [],
): Promise<boolean> {
  try {
    done(await ipc.workSend({ id: w.id, prompt, files, threads: null }));
    toast('info', 'Sent to Claude.');
    return true;
  } catch (err) {
    refused(err, 'Send to Claude');
    return false;
  }
}

/** Work item owning a PR, adopting a PR made outside Kelta first (FLOW §3.3 `f`). */
async function itemForReview(review: ReviewRef, projectId: ProjectId): Promise<WorkItem | null> {
  try {
    const plan = await ipc.workPlan({ project_id: projectId, source: { kind: 'review', review } });
    if (plan.existing)
      return work.get(plan.existing) ?? work.upsert(await ipc.workResume({ id: plan.existing }));
    if (!plan.adopt_pr) {
      toasts.warn('Fix with Claude needs your own pull request.');
      return null;
    }
    return work.upsert(await ipc.workStart({ plan }));
  } catch (err) {
    toasts.error(err, 'Fix with Claude');
    return null;
  }
}

// ---- registry --------------------------------------------------------------------------------

registerAction('work.fix', async (args) => {
  const review = args?.review as ReviewRef | undefined;
  const w =
    review && args?.project_id ? await itemForReview(review, args.project_id as ProjectId) : target(args);
  if (!w) return;
  if (!w.pr_url) {
    toasts.info('Fix with Claude needs a pull request. Ship first.');
    return;
  }
  ui.openSheet('fix', { id: w.id });
});

registerAction('work.push', async (args) => {
  const w = target(args);
  if (w) await push(w, false);
});

registerAction('work.force_push', (args) => {
  const w = target(args);
  if (!w) return;
  const sha = w.rebase?.remote_sha;
  if (!sha) {
    toast('warn', 'Force push is only offered to rewrite your own rebased commits.');
    return;
  }
  const remote = remoteOf(w);
  openWorkDialog({
    title: 'Force push',
    tone: 'danger',
    text: `Rewrites ${w.branch} on ${remote} (${prLabel(w)}). The lease checks ${remote} is still at ${sha.slice(0, 7)}.`,
    actions: [
      { label: 'Cancel', variant: 'ghost' },
      { label: 'Force push', command: 'work.force_push.confirmed', args: { id: w.id }, variant: 'danger' },
    ],
  });
});

// Reached only from the confirmation dialog above (not in the catalog, never bulk).
registerAction('work.force_push.confirmed', async (args) => {
  const w = target(args);
  if (w) await push(w, true);
});

registerAction('work.rebase', async (args) => {
  const w = target(args);
  if (w) await rebase(w, args?.onto === 'remote_branch' ? 'remote_branch' : 'base', args?.no_fetch === true);
});

for (const [id, kind, label] of [
  ['work.rebase_continue', 'continue', 'Continue rebase'],
  ['work.rebase_abort', 'abort', 'Abort rebase'],
] as const) {
  registerAction(id, async (args) => {
    const w = target(args);
    if (!w) return;
    try {
      const out = done(await ipc.workRebase({ id: w.id, op: { kind } }));
      if (kind === 'abort') toast('info', 'Rebase aborted.');
      else if (out.rebase && out.rebase.total > 0)
        toast('warn', 'Rebase still stopped: resolve the conflicts first.');
      else toast('info', out.rebase ? 'Rebase done. Force push to update the PR.' : 'Rebase done.');
    } catch (err) {
      refused(err, label);
    }
  });
}

registerAction('work.rebase_conflicts', async (args) => {
  const w = target(args);
  const files = w?.rebase?.conflicts ?? [];
  if (!w || files.length === 0) return;
  // shortcut: one `:edit` per file (first one shown last) instead of `:args`; add an nvim `:args`
  // RPC if buffer-list navigation is not enough.
  try {
    for (const f of [...files].reverse()) {
      await ipc.editorOpen({ target: { kind: 'work_item', id: w.id }, path: f, line: null });
    }
  } catch (err) {
    toasts.error(err, 'Open conflicts in nvim');
  }
});

registerAction('work.rebase_ask_claude', async (args) => {
  const w = target(args);
  if (!w?.rebase) return;
  await sendToClaude(w, promptTemplate('conflicts'), [
    { name: 'conflicts.md', content: conflictsMarkdown(w) },
  ]);
});

registerAction('work.ask_commit', async (args) => {
  const w = target(args);
  if (w) await sendToClaude(w, COMMIT_PROMPT);
});

registerAction('work.rerequest_review', async (args) => {
  const w = target(args);
  if (!w) return;
  try {
    const who = await ipc.workRerequestReview({ id: w.id });
    const fresh = work.get(w.id) ?? w;
    toast(
      'info',
      `Asked ${who.join(', ')} to review again.`,
      fresh.sent_threads?.length
        ? { label: 'Resolve sent threads', command: 'work.resolve_threads', args: { id: w.id } }
        : null,
    );
  } catch (err) {
    toasts.error(err, 'Re-request review');
  }
});

registerAction('work.resolve_threads', async (args) => {
  const w = target(args);
  if (!w) return;
  const n = w.sent_threads?.length ?? 0;
  try {
    done(await ipc.workResolveSentThreads({ id: w.id }));
    toast('info', `Resolved ${n} thread${n === 1 ? '' : 's'}.`);
  } catch (err) {
    toasts.error(err, 'Resolve sent threads');
  }
});

// Lease rejected: the push already fetched; show the item and its fresh status.
registerAction('work.show', async (args) => {
  const w = target(args);
  if (!w) return;
  try {
    done(await ipc.workResume({ id: w.id }));
  } catch (err) {
    toasts.error(err, 'Show work item');
  }
});
