// Reviews (#401): one list in sections by what I can do, stacks in merge order, and the review state in
// words. Pure.

import { msg, t, tn } from '$lib/i18n/index.svelte';
import type { Review, ReviewItem } from '$lib/gen';
import { reviewKey } from '$lib/stores/reducers';

export type SectionId = 'asked' | 'team' | 'drafts' | 'mine';

export const SECTIONS: {
  id: SectionId;
  label: string;
  folded?: boolean;
  quiet?: boolean;
  setting?: string;
}[] = [
  { id: 'asked', label: msg('Asked of you') },
  { id: 'team', label: msg('Team, needs a reviewer'), setting: 'reviews.include_team_requests' },
  { id: 'drafts', label: msg('Team drafts'), folded: true, quiet: true, setting: 'reviews.include_drafts' },
  { id: 'mine', label: msg('Yours, as reviewers see them'), quiet: true },
];

/** Where a PR sits in its stack: `depth` 0 is a base; `parent` is the PR it merges into. */
export interface Stacked {
  item: ReviewItem;
  depth: number;
  parent: ReviewItem | null;
  /** PRs stacked on this one, itself included (1: no stack). */
  size: number;
}

/** PRs in merge order: a PR whose base is another listed PR's head follows it, one step deeper. */
export function stackOrder(items: readonly ReviewItem[]): Stacked[] {
  const head = (i: ReviewItem) => `${i.review.ref.account}|${i.review.ref.repo}|${i.review.source_branch}`;
  const base = (i: ReviewItem) => `${i.review.ref.account}|${i.review.ref.repo}|${i.review.target_branch}`;
  const byHead = new Map(items.map((i) => [head(i), i]));
  const parentOf = (i: ReviewItem) => {
    const p = byHead.get(base(i));
    return p && p !== i ? p : null;
  };
  const children = new Map<ReviewItem, ReviewItem[]>();
  for (const i of items) {
    const p = parentOf(i);
    if (p) children.set(p, [...(children.get(p) ?? []), i]);
  }
  const size = (i: ReviewItem, seen: Set<ReviewItem>): number =>
    seen.has(i) ? 0 : (seen.add(i), 1 + (children.get(i) ?? []).reduce((n, c) => n + size(c, seen), 0));
  const out: Stacked[] = [];
  const seen = new Set<ReviewItem>();
  const walk = (i: ReviewItem, depth: number, parent: ReviewItem | null) => {
    if (seen.has(i)) return; // a cycle of bases: each PR once
    seen.add(i);
    out.push({ item: i, depth, parent, size: size(i, new Set()) });
    for (const c of children.get(i) ?? []) walk(c, depth + 1, i);
  };
  for (const i of items) if (!parentOf(i)) walk(i, 0, null);
  for (const i of items) walk(i, 0, null);
  return out;
}

/** The sections' PRs: asked of me, the team's (not mine) by draft, then mine; each PR once. */
export function sectionItems(
  asked: readonly ReviewItem[],
  team: readonly ReviewItem[],
  mine: readonly ReviewItem[],
): Record<SectionId, ReviewItem[]> {
  const taken = new Set([...asked, ...mine].map((i) => reviewKey(i.review.ref)));
  const others = team.filter((i) => !taken.has(reviewKey(i.review.ref)));
  return {
    asked: [...asked],
    team: others.filter((i) => !i.review.draft),
    drafts: others.filter((i) => i.review.draft),
    mine: [...mine],
  };
}

/** "No review yet, nobody requested", "1 approval, 2 reviewers still requested". */
export function reviewState(rv: Pick<Review, 'reviewers' | 'waiting_on' | 'decision'>): string {
  const n = (s: string) => rv.reviewers.filter((r) => r.state === s).length;
  const [approvals, changes, comments] = [n('approved'), n('changes_requested'), n('commented')];
  const done = [
    changes > 0 ? tn('{n} change request', '{n} change requests', changes) : '',
    approvals > 0 ? tn('{n} approval', '{n} approvals', approvals) : '',
    comments > 0 ? tn('{n} comment', '{n} comments', comments) : '',
  ].filter(Boolean);
  // A host that lists no reviewers still says approved / changes requested.
  if (done.length === 0 && rv.decision === 'approved') done.push(t('Approved'));
  if (done.length === 0 && rv.decision === 'changes_requested') done.push(t('Changes requested'));
  const asked = rv.waiting_on.length;
  if (done.length === 0)
    return asked > 0
      ? tn('{n} reviewer requested, none reviewed', '{n} reviewers requested, none reviewed', asked)
      : t('No review yet, nobody requested');
  const rest =
    asked > 0
      ? tn('{n} reviewer still requested', '{n} reviewers still requested', asked)
      : t('nobody requested');
  return [...done, rest].join(', ');
}

/** The row's state: draft or the review state, then where it sits in its stack. */
export function rowState(s: Stacked): string {
  const rv = s.item.review;
  const stack = s.parent
    ? t('merges into #{n}', { n: s.parent.review.ref.number })
    : s.size > 1
      ? t('base of a stack of {n}', { n: s.size })
      : '';
  return [rv.draft ? t('Draft') : reviewState(rv), stack].filter(Boolean).join(', ');
}

/** Days since the PR opened (its last update when the host gives no open date). */
export function openDays(rv: Pick<Review, 'created_at' | 'updated_at'>, now = Date.now()): number {
  const at = Date.parse(rv.created_at ?? rv.updated_at);
  return Number.isNaN(at) ? 0 : Math.max(0, Math.floor((now - at) / 86_400_000));
}

/** Team PRs open longer than this are highlighted. */
export const STALE_DAYS = 14;

/** My PR's next step as the Queue words it (Fix with Claude, Nudge Bob); '' when it waits on nobody. */
export function ownStep(rv: Pick<Review, 'decision' | 'ci' | 'waiting_on'>): string {
  if (rv.decision === 'changes_requested') return t('Fix with Claude');
  if (rv.ci === 'failure' || rv.ci === 'error') return t('Send failures to Claude');
  const [who] = rv.waiting_on;
  return who ? t('Nudge {name}', { name: who }) : '';
}
