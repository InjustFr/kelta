// Ticket sources (`[[sources]]`, global): add a discovered `SourceHit` to a source's board, spot one
// that is already there, create a source from a hit, and save the list to the global settings.

import { t } from '$lib/i18n/index.svelte';
import type { AccountKind, JsonValue, Source, SourceHit, TrackerBinding, TrackerView } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { settings, toasts } from '$lib/stores';

import { emptyBinding } from '../../onboarding/draft';
import { slugify, uniqueId } from './accounts';

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
  const view: TrackerView = {
    ...hit.view,
    id: uniqueId(
      hit.view.id,
      base.views.map((v) => v.id),
    ),
    who: 'mine',
    account: account === base.account ? null : account,
  };
  return { ...base, views: [...base.views, view] };
}

/** A new source labelled after the picked board, feeding `projectId` when given (none is valid). */
export function newSource(
  taken: readonly Source[],
  account: string,
  hit: SourceHit,
  projectId?: string,
): Source {
  const id = uniqueId(
    slugify(hit.label) || 'source',
    taken.map((s) => s.id),
  );
  const projects = projectId ? [projectId] : [];
  return { id, label: hit.label, projects, tracker: withSource(null, account, hit), routes: [] };
}

/** Replaces the global `[[sources]]` list (sources are global only: one layer holds them all). */
export async function saveSources(next: readonly Source[]): Promise<void> {
  // entries may be store proxies; IPC needs plain data
  const value = $state.snapshot(next) as unknown as JsonValue;
  settings.accept(null, await ipc.settingsSet({ layer: 'global', path: 'sources', value }));
}

/** The global sources with `id` replaced by `f(it)`. */
export const withEdited = (all: readonly Source[], id: string, f: (s: Source) => Source): Source[] =>
  all.map((s) => (s.id === id ? f(s) : s));

/** Saves source `id` as `f(it)`; a failure is a toast. */
export async function editSource(id: string, f: (s: Source) => Source): Promise<void> {
  try {
    await saveSources(withEdited(settings.value()?.sources ?? [], id, f));
  } catch (err) {
    toasts.error(err, t('Could not save the ticket source'));
  }
}

const ITERATION: Record<string, string> = {
  jira: 'sprint',
  linear: 'cycle',
  gitlab: 'milestone',
  redmine: 'version',
};

/** What the provider calls its current iteration (SPEC decision 3); null where `current_iteration`
 *  does nothing: gitea, GitHub repo views (only project_v2 has iterations), kind not loaded yet. */
export function iterationWord(kind: AccountKind | undefined, v?: TrackerView): string | null {
  if (!kind || kind === 'gitea' || (kind === 'github' && v && !v.project_v2)) return null;
  return ITERATION[kind] ?? 'iteration';
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
