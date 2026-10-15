import { msg } from '$lib/i18n/index.svelte';

import type { CopyTable } from './index';

const STARTING = msg('Starting');
const FOLDER = msg('Branch folder');
const PR = msg('Pull request');
const MERGE = msg('After merge');
const LIMITS = msg('Limits');
/** Group headings in the order work happens; Worktree.svelte links to each. */
export const STAGES = [STARTING, FOLDER, PR, MERGE, LIMITS] as const;

const FALLBACK = msg('Fallback the default steps are built from, for a source without steps of its own.');

export default {
  // roots and intermediate groups: not shown as rows, required by the COPY_DONE test
  worktree: { label: msg('Branch folders') },
  work: { label: msg('Work items') },
  tickets: { label: msg('Tickets') },
  ports: { label: msg('Ports') },
  session_templates: {
    label: msg('Session templates'),
    help: msg('Session layouts a work item or a review can open.'),
  },
  'work.on_start': { label: msg('When work starts') },
  'work.on_pr': { label: msg('When a pull request opens') },
  'work.on_merge': { label: MERGE },
  'work.pr': { label: PR },

  'work.on_start.assign_me': {
    group: STARTING,
    order: 0,
    label: msg('Assign the ticket to me'),
    help: msg('When work starts, the ticket is assigned to you on the tracker.'),
  },
  'work.plan_preview': {
    group: STARTING,
    order: 1,
    label: msg('Review the plan before starting'),
    help: msg('Shows what Kelta will create first: branch, folder and sessions.'),
  },
  'work.finish_first': {
    group: STARTING,
    order: 2,
    label: msg('List unfinished work first'),
    help: msg('Starting something new while other work is unfinished shows that work first.'),
  },
  'work.default_template': {
    group: STARTING,
    order: 3,
    label: msg('Sessions a new work item opens'),
    help: msg('The session template a new work item starts with. A project can set its own.'),
  },

  'worktree.branch_template': {
    keywords: [msg('branch name')],
    group: FOLDER,
    order: 10,
    label: msg('Branch name'),
    help: msg('Checked as a valid git branch name.'),
  },
  'worktree.root': {
    keywords: [msg('worktree folder'), msg('location')],
    group: FOLDER,
    order: 11,
    label: msg('Where branch folders live'),
    help: msg('Keep it outside the repository.'),
  },
  'worktree.include': {
    group: FOLDER,
    order: 12,
    label: msg('Files copied into each branch folder'),
    help: msg(
      'Untracked files a fresh checkout needs, such as .env. Patterns in .worktreeinclude are added.',
    ),
  },
  'worktree.setup': {
    group: FOLDER,
    order: 13,
    label: msg('Setup commands'),
    help: msg('Run in a visible pane when the branch folder is created.'),
  },
  'worktree.setup_blocking': {
    group: FOLDER,
    order: 14,
    label: msg('Claude waits for setup to finish'),
    help: msg('Claude starts only once every setup command succeeds.'),
  },
  'worktree.teardown': {
    group: FOLDER,
    order: 15,
    label: msg('Teardown command'),
    help: msg('Runs in the branch folder on Finish. If it fails, Finish stops.'),
  },

  'work.pr.title_template': {
    keywords: [msg('pr'), msg('pull request title')],
    group: PR,
    order: 20,
    label: msg('Title'),
    help: msg('Title of the pull requests Kelta opens.'),
  },
  'work.pr.body_template': {
    keywords: [msg('pr'), msg('pull request description')],
    group: PR,
    order: 21,
    label: msg('Description'),
    help: msg('{closes} adds Closes #n for GitHub issues.'),
  },
  'work.pr.draft': {
    keywords: [msg('pr')],
    group: PR,
    order: 22,
    label: msg('Open as a draft'),
    help: msg('New pull requests start as drafts.'),
  },
  'work.pr.merge_method': {
    keywords: [msg('pr'), msg('squash')],
    group: PR,
    order: 23,
    label: msg('Merge method'),
    help: msg('What Merge when ready uses unless you pick another.'),
    options: { merge: { label: msg('Merge commit') } },
  },

  'work.on_merge.offer_cleanup': {
    group: MERGE,
    order: 30,
    label: msg('Offer to delete the branch folder'),
    help: msg('Once the pull request is merged, Finish offers to remove the branch folder.'),
  },

  'tickets.wip_limit': {
    group: LIMITS,
    order: 40,
    label: msg('Work-in-progress limit'),
    help: msg('The Doing header warns above it. Nothing is blocked.'),
    keywords: [msg('WIP')],
  },
  'work.return_brief_after_mins': {
    group: LIMITS,
    order: 41,
    label: msg('Show the catch-up strip after, in minutes away'),
    help: msg(
      'Coming back to a work item shows its next: note, the last message from Claude and what changed. 0 turns it off.',
    ),
  },

  // Advanced (sections.ts ADVANCED)
  'work.on_start.transition_to': { label: msg('Ticket move when work starts'), help: FALLBACK },
  'work.on_start.comment': { label: msg('Comment when work starts'), help: FALLBACK },
  'work.on_pr.transition_to': { label: msg('Ticket move when a pull request opens'), help: FALLBACK },
  'work.on_pr.comment': { label: msg('Comment when a pull request opens'), help: FALLBACK },
  'work.on_merge.transition_to': { label: msg('Ticket move after merge'), help: FALLBACK },
  'work.pipeline': {
    label: msg('Default steps'),
    help: msg('Steps for a source that sets none. Empty builds them from the moves and comments above.'),
  },
  'work.scratch_branch_template': {
    label: msg('Scratch branch name'),
    help: msg('Branch of a work item started without a ticket; {slug} is its first line as a slug.'),
  },
  'work.review_template': {
    label: msg('Sessions a review opens'),
    help: msg('The session template a pull request review starts with.'),
  },
  'worktree.slug_max': {
    label: msg('Branch name length'),
    help: msg('Longest title slug in branch and folder names, in characters.'),
  },
  'worktree.type_map': {
    label: msg('Ticket types in branch names'),
    help: msg(
      'Maps a ticket type such as bug or story to the type part of the branch name; default covers the rest.',
    ),
  },
  'worktree.env_template': {
    label: msg('.env template'),
    help: msg(
      'A file ending in .kelta, such as .env.kelta, written without .kelta when work starts, with its ports filled in. An existing different file is kept.',
    ),
  },
  'worktree.cleanup': {
    label: msg('Branch folder cleanup'),
    help: msg(
      'Ask, delete automatically or keep the branch folder once its pull request is merged or its ticket is done.',
    ),
  },
  'ports.range': {
    label: msg('Port range'),
    help: msg('First-last: each work item gets the next free block of 10 ports. Empty gives no ports.'),
  },
  'worktree.fetch_timeout_secs': {
    label: msg('Fetch timeout, in seconds'),
    help: msg('How long starting work waits for git fetch.'),
  },
} satisfies CopyTable;
