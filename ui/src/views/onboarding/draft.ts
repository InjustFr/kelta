// "New project" draft model: validation, slugs and the minimal tracker binding built from a
// detected hint. The authoritative validation is the backend's; this keeps the sheet honest.

import type { AccountKind, ProjectDraft, RepoDraft, TrackerBinding, TrackerView } from '$lib/gen';

export const ID_RE = /^[a-z0-9-]{1,40}$/;

export function slugify(name: string): string {
  const s = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40)
    .replace(/-+$/g, '');
  return s || 'project';
}

export interface DraftIssues {
  id?: string;
  name?: string;
  repos?: string;
  /** Per-repo messages keyed by index. */
  repo: Record<number, string>;
}

export function validateDraft(d: ProjectDraft, existingIds: readonly string[]): DraftIssues {
  const out: DraftIssues = { repo: {} };
  if (!ID_RE.test(d.suggested_id)) out.id = 'Use 1 to 40 characters from a-z, 0-9 and "-".';
  else if (d.suggested_id === 'home') out.id = '"home" is reserved for the built-in Home project.';
  else if (existingIds.includes(d.suggested_id)) out.id = 'A project with this id already exists.';
  if (!d.name.trim()) out.name = 'Give the project a name.';
  if (d.repos.length === 0) out.repos = 'Add at least one repository.';
  else if (d.repos.filter((r) => r.primary).length !== 1)
    out.repos = 'Exactly one repository must be primary.';
  const seen = new Set<string>();
  d.repos.forEach((r, i) => {
    if (!/^[A-Za-z0-9_.-]+$/.test(r.id)) out.repo[i] = 'Repository id: letters, digits, "-", "_" and ".".';
    else if (seen.has(r.id)) out.repo[i] = 'Duplicate repository id.';
    else if (!r.path.trim()) out.repo[i] = 'Path is required.';
    seen.add(r.id);
  });
  return out;
}

export function hasIssues(i: DraftIssues): boolean {
  return !!(i.id || i.name || i.repos || Object.keys(i.repo).length > 0);
}

/** Makes `index` the only primary repo. */
export function setPrimary(repos: readonly RepoDraft[], index: number): RepoDraft[] {
  return repos.map((r, i) => ({ ...r, primary: i === index }));
}

/** Removes a repo, keeping a primary if the removed one was it. */
export function removeRepo(repos: readonly RepoDraft[], index: number): RepoDraft[] {
  const next = repos.filter((_, i) => i !== index);
  if (next.length > 0 && !next.some((r) => r.primary)) next[0] = { ...next[0]!, primary: true };
  return next;
}

export function newRepo(n: number, primary: boolean): RepoDraft {
  return {
    id: n === 0 ? 'main' : `repo-${n + 1}`,
    path: '',
    primary,
    remote: 'origin',
    base: 'main',
    remote_url: null,
    code_host: null,
  };
}

/** First view of a tracker binding, per account kind. `hint` is the detected key / repo. */
export function defaultView(kind: AccountKind | string, hint: string | null): TrackerView {
  const base: TrackerView = {
    id: 'mine',
    label: 'My tickets',
    jql: null,
    board_id: null,
    project_id: null,
    query_id: null,
    assigned_to: null,
    status: null,
    repo: null,
    search: null,
    project_v2: null,
    project: null,
    team: null,
    scope: null,
    labels: null,
    workflow_scope: null,
  };
  switch (kind) {
    case 'jira':
      return {
        ...base,
        jql: hint
          ? `project = ${hint} AND assignee = currentUser() AND statusCategory != Done`
          : 'assignee = currentUser() AND statusCategory != Done',
      };
    case 'redmine':
      return { ...base, project_id: hint, assigned_to: 'me' };
    case 'github':
      return { ...base, repo: hint };
    case 'gitlab':
      return { ...base, project: hint, scope: 'assigned_to_me' };
    case 'linear':
      return { ...base, team: hint, scope: 'assigned_to_me' };
    default:
      return base;
  }
}

export function bindTracker(
  account: string,
  kind: AccountKind | string,
  hint: string | null,
): TrackerBinding {
  return {
    account,
    views: [defaultView(kind, hint)],
    columns: [],
    status_map: { start: null, review: null, done: null },
    repo_rules: [],
  };
}

/** Default project colours (DESIGN §6.12): stored hex = light value; the chip shows `--k-swatch-<name>`. */
export const PALETTE = [
  { name: 'harbor', hex: '#3e7cb1' },
  { name: 'moss', hex: '#5e8c4a' },
  { name: 'ochre', hex: '#b88a2e' },
  { name: 'brick', hex: '#b5533c' },
  { name: 'plum', hex: '#8a5a9e' },
  { name: 'teal', hex: '#2f8c86' },
  { name: 'rose', hex: '#b85278' },
  { name: 'slate', hex: '#5f6b7a' },
  { name: 'olive', hex: '#85863a' },
  { name: 'cobalt', hex: '#4b5fc4' },
] as const;
