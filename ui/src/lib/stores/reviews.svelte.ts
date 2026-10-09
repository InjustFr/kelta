// Review lists (per scope + kind) and details. `reviews.changed` refetches the affected lists and
// records the newly requested review keys (badges).

import type { ReviewDetail, ReviewItem, ReviewKind, ReviewPage, ReviewRef, Scope, UiEvent } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

import { idle, settle, type Loadable } from './loadable';
import { reviewKey, scopeAffects, scopeKey } from './reducers';

export interface ReviewList extends Loadable<ReviewPage> {
  scope: Scope;
  kind: ReviewKind;
}

export function reviewListKey(scope: Scope, kind: ReviewKind): string {
  return `${scopeKey(scope)}|${kind}`;
}

export class ReviewsStore {
  lists = $state<Record<string, ReviewList>>({});
  details = $state<Record<string, Loadable<ReviewDetail>>>({});
  /** Keys announced as new by `reviews.changed` and not yet seen. */
  newKeys = $state<string[]>([]);

  list(scope: Scope, kind: ReviewKind): ReviewList {
    return this.lists[reviewListKey(scope, kind)] ?? { ...idle<ReviewPage>(), scope, kind };
  }

  items(scope: Scope, kind: ReviewKind): ReviewItem[] {
    return this.list(scope, kind).data?.items ?? [];
  }

  isNew(ref: ReviewRef): boolean {
    return this.newKeys.includes(reviewKey(ref));
  }

  markSeen(ref: ReviewRef): void {
    const key = reviewKey(ref);
    if (this.newKeys.includes(key)) this.newKeys = this.newKeys.filter((k) => k !== key);
  }

  async load(scope: Scope, kind: ReviewKind, refresh = false): Promise<ReviewList> {
    const key = reviewListKey(scope, kind);
    const prev = this.list(scope, kind);
    this.lists = { ...this.lists, [key]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.reviewList({ scope, kind, refresh }), {
      stale: (p) => p.stale,
    });
    const entry: ReviewList = { ...next, scope, kind };
    this.lists = { ...this.lists, [key]: entry };
    return entry;
  }

  async loadDetail(ref: ReviewRef): Promise<Loadable<ReviewDetail>> {
    const key = reviewKey(ref);
    const prev = this.details[key] ?? idle<ReviewDetail>();
    this.details = { ...this.details, [key]: { ...prev, loading: true } };
    const next = await settle(prev, () => ipc.reviewGet({ review: ref }));
    this.details = { ...this.details, [key]: next };
    return next;
  }

  /**
   * Re-reads `my_state` and the heads of the given PRs through `reviewGet` (CodeHost::get) and
   * patches the cached rows. Called on Now open and window focus; there is no polling.
   */
  async refreshState(refs: ReviewRef[]): Promise<void> {
    const details = await Promise.all(refs.map((r) => this.loadDetail(r)));
    const fresh = Object.fromEntries(
      details.flatMap((d) => (d.data ? [[reviewKey(d.data.review.ref), d.data.review]] : [])),
    );
    const patch = (i: ReviewItem): ReviewItem => {
      const f = fresh[reviewKey(i.review.ref)];
      if (!f) return i;
      const { my_state, head_sha, reviewed_head } = f;
      return { ...i, review: { ...i.review, my_state, head_sha, reviewed_head } };
    };
    this.lists = Object.fromEntries(
      Object.entries(this.lists).map(([k, l]) => [
        k,
        l.data ? { ...l, data: { ...l.data, items: l.data.items.map(patch) } } : l,
      ]),
    );
  }

  apply(ev: UiEvent): void {
    if (ev.type !== 'reviews.changed') return;
    if (ev.new_keys.length > 0) {
      const add = ev.new_keys.map(reviewKey).filter((k) => !this.newKeys.includes(k));
      if (add.length > 0) this.newKeys = [...this.newKeys, ...add];
    }
    const lists: Record<string, ReviewList> = {};
    let touched = false;
    for (const [k, l] of Object.entries(this.lists)) {
      const hit = scopeAffects(ev.scope, l.scope);
      lists[k] = hit ? { ...l, invalidated: true } : l;
      touched ||= hit;
    }
    if (!touched) return;
    this.lists = lists;
    void this.refreshInvalidated();
  }

  async refreshInvalidated(): Promise<void> {
    const stale = Object.values(this.lists).filter((l) => l.invalidated && !l.loading);
    await Promise.all(stale.map((l) => this.load(l.scope, l.kind, false)));
  }
}
