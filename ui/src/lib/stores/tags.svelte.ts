// Local tags (#349): my reminders on tickets and pull requests, defined in Settings. Kelta-local
// (`tags_get` / `tags_set`), never sent to a tracker or code host. Items are keyed like the rest of
// the UI: `ticketKey` (account:key) and `reviewKey` (account:repo#number).

import type { LocalTag, LocalTags } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

export type TagKind = 'tickets' | 'prs';

// shortcut: each window loads the set once at start; a detached window (#342) sees another window's
// changes after a restart. Add a UiEvent if that bites.
export class TagsStore {
  value = $state<LocalTags>({ tags: [], tickets: {}, prs: {} });
  /** A failed save (the set is rolled back); index.ts toasts it. */
  onSaveError: (err: unknown) => void = () => {};

  async load(): Promise<void> {
    this.value = await ipc.tagsGet({});
  }

  /** The tags on an item, in the Settings order. */
  of(kind: TagKind, key: string): LocalTag[] {
    const names = this.value[kind][key] ?? [];
    return this.value.tags.filter((t) => names.includes(t.name));
  }

  has(kind: TagKind, key: string, name: string): boolean {
    return this.value[kind][key]?.includes(name) ?? false;
  }

  /** `name` while that tag exists (a filter on a deleted tag lets everything through). */
  live(name: string | null): string | null {
    return name !== null && this.value.tags.some((t) => t.name === name) ? name : null;
  }

  /** Every tag with how many of `keys` carry it (the toolbars' filter chips). */
  counts(kind: TagKind, keys: readonly string[]): { tag: LocalTag; n: number }[] {
    const on = this.value[kind];
    return this.value.tags.map((tag) => ({ tag, n: keys.filter((k) => on[k]?.includes(tag.name)).length }));
  }

  /** Items carrying `name`, tickets and pull requests together (the delete confirmation). */
  uses(name: string): number {
    return [...Object.values(this.value.tickets), ...Object.values(this.value.prs)].filter((v) =>
      v.includes(name),
    ).length;
  }

  toggle(kind: TagKind, key: string, name: string): Promise<void> {
    const cur = this.value[kind][key] ?? [];
    const next = cur.includes(name) ? cur.filter((n) => n !== name) : [...cur, name];
    return this.#save({ ...this.value, [kind]: { ...this.value[kind], [key]: next } });
  }

  /** Adds a tag (`at` null) or replaces the one at `at`; a rename keeps it on its items. */
  put(at: number | null, tag: LocalTag): Promise<void> {
    const old = at === null ? null : (this.value.tags[at] ?? null);
    const tags =
      at === null ? [...this.value.tags, tag] : this.value.tags.map((t, i) => (i === at ? tag : t));
    const rename = (m: Record<string, string[]>): Record<string, string[]> =>
      old && old.name !== tag.name
        ? Object.fromEntries(
            Object.entries(m).map(([k, v]) => [k, v.map((n) => (n === old.name ? tag.name : n))]),
          )
        : m;
    return this.#save({ tags, tickets: rename(this.value.tickets), prs: rename(this.value.prs) });
  }

  /** Deletes a tag everywhere (the core drops it from the items). */
  remove(name: string): Promise<void> {
    return this.#save({ ...this.value, tags: this.value.tags.filter((t) => t.name !== name) });
  }

  async #save(next: LocalTags): Promise<void> {
    const prev = this.value;
    this.value = next;
    try {
      this.value = await ipc.tagsSet({ tags: $state.snapshot(next) });
    } catch (err) {
      this.value = prev;
      this.onSaveError(err);
    }
  }
}
