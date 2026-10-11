// Local tags (#349): my reminders on tickets and pull requests, defined in Settings. Kelta-local
// (`tags_get` / `tags_set`), never sent to a tracker or code host. Items are keyed like the rest of
// the UI: `ticketKey` (account:key) and `reviewKey` (account:repo#number).

import type { LocalTag, LocalTags } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';

export type TagKind = 'tickets' | 'prs';

// shortcut: a window reads the set at start and again before each change (so a save never overwrites
// another window's, #342), but it only shows another window's change after its own next change or a
// restart; the read-then-write window is not atomic. Add a UiEvent if that bites.
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
    return this.#save((v) => {
      const cur = v[kind][key] ?? [];
      const next = cur.includes(name) ? cur.filter((n) => n !== name) : [...cur, name];
      return { ...v, [kind]: { ...v[kind], [key]: next } };
    });
  }

  /** Adds a tag (`at` null) or replaces the one at `at`; a rename keeps it on its items. */
  put(at: number | null, tag: LocalTag): Promise<void> {
    return this.#save((v) => {
      const old = at === null ? null : (v.tags[at] ?? null);
      const tags = at === null ? [...v.tags, tag] : v.tags.map((t, i) => (i === at ? tag : t));
      const rename = (m: Record<string, string[]>): Record<string, string[]> =>
        old && old.name !== tag.name
          ? Object.fromEntries(
              Object.entries(m).map(([k, x]) => [k, x.map((n) => (n === old.name ? tag.name : n))]),
            )
          : m;
      return { tags, tickets: rename(v.tickets), prs: rename(v.prs) };
    });
  }

  /** Deletes a tag everywhere (the core drops it from the items). */
  remove(name: string): Promise<void> {
    return this.#save((v) => ({ ...v, tags: v.tags.filter((t) => t.name !== name) }));
  }

  #queue: Promise<void> = Promise.resolve();

  /** Applies `change` to the freshly stored set, one save at a time. */
  #save(change: (v: LocalTags) => LocalTags): Promise<void> {
    return (this.#queue = this.#queue.then(() => this.#run(change)));
  }

  async #run(change: (v: LocalTags) => LocalTags): Promise<void> {
    const prev = this.value;
    try {
      const next = change(await ipc.tagsGet({}));
      this.value = next;
      this.value = await ipc.tagsSet({ tags: $state.snapshot(next) });
    } catch (err) {
      this.value = prev;
      this.onSaveError(err);
    }
  }
}
