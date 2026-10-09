// Pure helpers shared by the work views (tickets, reviews, inbox, start-work sheet, work header).

import type { AccountError, JsonValue, KeltaError, StartWorkPlan, Transition } from '$lib/gen';

// ---- plan validation ------------------------------------------------------------------------

/** Approximation of `git check-ref-format --branch`. Returns an error message or null. */
export function validateBranch(name: string): string | null {
  const n = name;
  if (n.trim() === '') return 'Branch name is required';
  if (n !== n.trim()) return 'Branch name must not start or end with a space';
  if (n.startsWith('-')) return 'Branch name must not start with "-"';
  if (n === '@') return 'Branch name must not be "@"';
  for (const ch of n) {
    const c = ch.codePointAt(0) ?? 0;
    if (c <= 0x20 || c === 0x7f || '~^:?*[\\'.includes(ch))
      return 'Branch name contains an invalid character';
  }
  if (n.includes('..')) return 'Branch name must not contain ".."';
  if (n.includes('@{')) return 'Branch name must not contain "@{"';
  if (n.includes('//') || n.startsWith('/') || n.endsWith('/')) return 'Branch name has an empty path part';
  if (n.endsWith('.')) return 'Branch name must not end with "."';
  for (const part of n.split('/')) {
    if (part.startsWith('.')) return 'Path parts must not start with "."';
    if (part.endsWith('.lock')) return 'Path parts must not end with ".lock"';
  }
  return null;
}

/** Scratch item title: the task's first non-empty line, 72 chars max (kelta-work `task_title`). */
export function taskTitle(task: string): string {
  const line =
    task
      .split('\n')
      .map((l) => l.trim())
      .find((l) => l !== '') ?? '';
  return [...line].slice(0, 72).join('').trimEnd();
}

/**
 * Live preview of `work.scratch_branch_template` for a task (kelta-work `slugify` + `{slug}`).
 * shortcut: NFKD stands in for the backend's transliteration table and only `{slug}` is
 * substituted; the backend renders the real branch when the field was not edited.
 */
export function scratchBranch(task: string, template: string, max: number): string {
  let slug = taskTitle(task)
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  if (max > 0 && slug.length > max) {
    const cut = slug.slice(0, max);
    const dash = cut.lastIndexOf('-');
    slug = slug[max] === '-' ? cut.replace(/-+$/, '') : dash > 0 ? cut.slice(0, dash) : cut;
  }
  return slug === '' ? '' : template.replaceAll('{slug}', slug);
}

export interface PlanErrors {
  branch: string | null;
  base: string | null;
  prompt: string | null;
  repo: string | null;
  comment: string | null;
}

export function validatePlan(plan: StartWorkPlan): PlanErrors {
  return {
    branch: validateBranch(plan.branch),
    base: plan.base.trim() === '' ? 'Base branch is required' : null,
    prompt: plan.claude.prompt.trim() === '' ? 'The prompt must not be empty' : null,
    repo:
      plan.repo_id === '' || (plan.repo_choices.length > 0 && !plan.repo_choices.includes(plan.repo_id))
        ? 'Pick a repository'
        : null,
    comment:
      plan.side_effects.comment !== null && plan.side_effects.comment.trim() === ''
        ? 'The comment is empty'
        : null,
  };
}

export function planValid(errors: PlanErrors): boolean {
  return Object.values(errors).every((e) => e === null);
}

const STEP_LABELS: Record<string, string> = {
  before_start: 'Check start triggers',
  fetch_ticket: 'Fetch ticket and write ticket.md',
  fetch_base: 'Fetch base branch',
  worktree: 'Create worktree',
  include_files: 'Copy worktree.include files',
  claude_files: 'Generate Claude settings and context',
  layout: 'Create tab and layout',
  setup: 'Run setup commands',
  editor: 'Start editor',
  claude: 'Start Claude',
  tracker_side_effects: 'Tracker updates (assign, move, comment)',
  persist: 'Save work item',
};

export const START_STEPS = Object.keys(STEP_LABELS);

export function stepLabel(step: string): string {
  return STEP_LABELS[step] ?? step.replace(/_/g, ' ');
}

// ---- error detail parsing -------------------------------------------------------------------

function obj(v: JsonValue | null | undefined): Record<string, JsonValue> | null {
  return v !== null && v !== undefined && typeof v === 'object' && !Array.isArray(v)
    ? (v as Record<string, JsonValue>)
    : null;
}

export interface FieldSpec {
  id: string;
  name: string;
  required: boolean;
  options: { value: string; label: string }[];
}

/** Parses the `detail.fields` of a `NeedsFields` error (strings or `{id,name,required,allowed_values}`). */
export function parseFields(detail: JsonValue | null | undefined): FieldSpec[] {
  const raw = obj(detail)?.fields;
  if (!Array.isArray(raw)) return [];
  const out: FieldSpec[] = [];
  for (const f of raw) {
    if (typeof f === 'string') {
      out.push({ id: f, name: f, required: true, options: [] });
      continue;
    }
    const o = obj(f);
    if (!o) continue;
    const id = String(o.id ?? o.key ?? o.name ?? '');
    if (id === '') continue;
    const allowed = Array.isArray(o.allowed_values) ? o.allowed_values : [];
    const options = allowed.flatMap((a) => {
      if (typeof a === 'string') return [{ value: a, label: a }];
      const ao = obj(a);
      if (!ao) return [];
      const value = String(ao.id ?? ao.value ?? ao.name ?? '');
      return value === '' ? [] : [{ value, label: String(ao.name ?? ao.label ?? value) }];
    });
    out.push({ id, name: String(o.name ?? id), required: o.required !== false, options });
  }
  return out;
}

/** Parses `detail.candidates` of an ambiguous `tracker_move` (full transitions or `{id,name}`). */
export function parseCandidates(detail: JsonValue | null | undefined): Transition[] {
  const raw = obj(detail)?.candidates;
  if (!Array.isArray(raw)) return [];
  const out: Transition[] = [];
  for (const c of raw) {
    const o = obj(c);
    if (!o) continue;
    const id = String(o.id ?? '');
    if (id === '') continue;
    const to = obj(o.to);
    out.push({
      id,
      name: String(o.name ?? to?.name ?? id),
      to: {
        id: String(to?.id ?? id),
        name: String(to?.name ?? o.name ?? id),
        category: (to?.category as Transition['to']['category']) ?? 'unknown',
      },
      needs_fields: o.needs_fields === true,
    });
  }
  return out;
}

/** `detail.files` of a `Dirty` error. */
export function parseDirtyFiles(detail: JsonValue | null | undefined): string[] {
  const raw = obj(detail)?.files;
  return Array.isArray(raw) ? raw.filter((f): f is string => typeof f === 'string') : [];
}

// ---- stale / account error banners ----------------------------------------------------------

export interface BannerInfo {
  tone: 'warn' | 'danger' | 'info';
  text: string;
  /** Offers the "Re-authenticate" action (opens the account settings). */
  reauth: boolean;
}

function hhmm(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** Message for one failed account next to aggregated data (SPEC §5). */
export function accountErrorBanner(e: AccountError, now: number = Date.now()): BannerInfo {
  const err = e.error;
  switch (err.code) {
    case 'needs_auth':
      return { tone: 'danger', text: `Account ${e.account_id} needs authentication`, reauth: true };
    case 'rate_limited':
      return {
        tone: 'warn',
        text: err.retry_after_ms
          ? `Account ${e.account_id} is rate limited, retrying at ${hhmm(now + err.retry_after_ms)}`
          : `Account ${e.account_id} is rate limited`,
        reauth: false,
      };
    case 'network':
    case 'timeout':
      return { tone: 'warn', text: `Offline: ${e.account_id} unreachable`, reauth: false };
    default:
      return { tone: 'warn', text: `Account ${e.account_id}: ${err.message}`, reauth: false };
  }
}

/** Banner for a failed load that kept stale data (or no data at all). */
export function loadErrorBanner(err: KeltaError, now: number = Date.now()): BannerInfo {
  return accountErrorBanner({ account_id: 'tracker', error: err }, now);
}

export function isAuthError(err: KeltaError | null | undefined): boolean {
  return err?.code === 'needs_auth';
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  return ((parts[0]?.[0] ?? '?') + (parts[1]?.[0] ?? '')).toUpperCase();
}

export type Tone = 'neutral' | 'accent' | 'ok' | 'warn' | 'danger' | 'info';

export function statusTone(category: string): Tone {
  switch (category) {
    case 'in_progress':
      return 'info';
    case 'in_review':
      return 'accent';
    case 'done':
      return 'ok';
    default:
      return 'neutral';
  }
}

/** Column a ticket status belongs to: by name first, then by category. */
export function columnFor<C extends { id: string; name: string; category: string; match_names: string[] }>(
  columns: readonly C[],
  status: { name: string; category: string },
): C | null {
  const lower = status.name.toLowerCase();
  return (
    columns.find(
      (c) => c.name.toLowerCase() === lower || c.match_names.some((n) => n.toLowerCase() === lower),
    ) ??
    columns.find((c) => c.category === status.category) ??
    null
  );
}

export function ciGlyph(state: string): { glyph: string; tone: Tone; label: string } {
  switch (state) {
    case 'success':
      return { glyph: '✓', tone: 'ok', label: 'CI passed' };
    case 'failure':
    case 'error':
      return { glyph: '✗', tone: 'danger', label: 'CI failed' };
    case 'pending':
      return { glyph: '●', tone: 'warn', label: 'CI running' };
    default:
      return { glyph: '–', tone: 'neutral', label: 'No CI' };
  }
}

export function decisionInfo(decision: string | null): { label: string; tone: Tone } | null {
  switch (decision) {
    case 'approved':
      return { label: 'approved', tone: 'ok' };
    case 'changes_requested':
      return { label: 'changes requested', tone: 'danger' };
    case 'review_required':
      return { label: 'review required', tone: 'warn' };
    default:
      return null;
  }
}

export function myStateInfo(state: string | null): { label: string; tone: Tone } | null {
  switch (state) {
    case 'approved':
      return { label: 'you approved', tone: 'ok' };
    case 'changes_requested':
      return { label: 'you requested changes', tone: 'danger' };
    case 'commented':
      return { label: 'you commented', tone: 'info' };
    case 'pending':
      return { label: 'awaiting you', tone: 'warn' };
    default:
      return null;
  }
}
