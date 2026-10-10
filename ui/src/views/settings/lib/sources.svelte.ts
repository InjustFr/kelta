// Ticket sources of a project (`[project.tracker].views`): add a discovered `SourceHit`, spot one
// that is already there, and save the binding through `project_update`.

import type { AccountKind, ProjectInfo, SourceHit, TrackerBinding, TrackerView } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { projects } from '$lib/stores';

import { emptyBinding } from '../../onboarding/draft';

/** Account a view reads from (`None` = the binding's). */
export const viewAccount = (b: TrackerBinding, v: TrackerView): string => v.account ?? b.account;

// Same source = same account and same provider query; id, label and who are the user's to change.
const sourceKey = (account: string, v: TrackerView): string =>
  JSON.stringify({ ...v, id: 0, label: 0, who: 0, account });

export function isAdded(b: TrackerBinding | null, account: string, hit: SourceHit): boolean {
  const key = sourceKey(account, hit.view);
  return !!b?.views.some((v) => sourceKey(viewAccount(b, v), v) === key);
}

/** `b` plus the hit's view (unique id, who = mine); creates the binding on `account` when there is none. */
export function withSource(b: TrackerBinding | null, account: string, hit: SourceHit): TrackerBinding {
  const base = b ?? emptyBinding(account);
  const taken = base.views.map((v) => v.id);
  let id = hit.view.id;
  for (let n = 2; taken.includes(id); n++) id = `${hit.view.id}-${n}`;
  const view: TrackerView = {
    ...hit.view,
    id,
    who: 'mine',
    account: account === base.account ? null : account,
  };
  return { ...base, views: [...base.views, view] };
}

export async function saveTracker(projectId: string, tracker: TrackerBinding): Promise<ProjectInfo> {
  const info = await ipc.projectUpdate({
    id: projectId,
    patch: {
      name: null,
      color: null,
      icon: null,
      default_template: null,
      repos: null,
      // views may be store proxies; IPC needs plain data
      tracker: $state.snapshot(tracker),
      remove_tracker: false,
    },
  });
  // The project.updated event does the same; applying now keeps "Added" right without waiting for it.
  projects.apply({ type: 'project.updated', project: info });
  return info;
}

const ITERATION: Record<string, string> = {
  jira: 'sprint',
  linear: 'cycle',
  gitlab: 'milestone',
  redmine: 'version',
};

/** What the provider calls its current iteration (SPEC decision 3); gitea has none. */
export function iterationWord(kind: AccountKind | undefined): string | null {
  return kind === 'gitea' ? null : (ITERATION[kind ?? ''] ?? 'iteration');
}

const NOT_TRACKERS: readonly AccountKind[] = ['bitbucket', 'plugin_codehost'];

/** Accounts that can hold tickets, plus any the binding already uses (settings may not be loaded). */
export function trackerAccountIds(
  accounts: Record<string, { kind: AccountKind }>,
  b: TrackerBinding | null,
): string[] {
  const ids = Object.entries(accounts)
    .filter(([, a]) => !NOT_TRACKERS.includes(a.kind))
    .map(([id]) => id);
  const used = b ? [b.account, ...b.views.map((v) => viewAccount(b, v))] : [];
  return [...ids, ...used].filter((id, i, all) => all.indexOf(id) === i);
}
