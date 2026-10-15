// Settings copy: the words the form, the settings search and ⌘K show for each key. One file per section
// (`copy/<section id>.ts`), so section work never edits the same file. A key without an entry falls back to
// its name (`titleOf`) and its Rust doc comment.
// Writing rules:
// - Name the effect, not the mechanism ("Let programs copy to the clipboard", not "OSC 52").
// - A toggle says what ON does.
// - Units in words ("in lines", "in seconds"), not "(ms)".
// - Help says what changes and when you would change it.
// - No TOML syntax, no internal names (struct, crate, SPEC §, `{ category = … }`).
// - label, help, group, keywords and option labels are msg() literals, translated in fr.json and de.json.

import type { SettingsSectionId } from '$app/registry';
import { currentPlatform } from '$lib/ui/format';

import accounts from './accounts';
import claude from './claude';
import editors from './editors';
import general from './general';
import keys from './keys';
import linuxGraphics from './linux-graphics';
import notifications from './notifications';
import performance from './performance';
import projects from './projects';
import reviews from './reviews';
import sources from './sources';
import terminal from './terminal';
import tools from './tools';
import windowCopy from './window';
import worktree from './worktree';

export interface CopyOption {
  label: string;
  help?: string;
}

export interface CopyEntry {
  label?: string;
  help?: string;
  /** Extra words the settings search and ⌘K match (not shown). */
  keywords?: string[];
  /** Heading over consecutive keys inside the section; give a group's keys consecutive `order`s. */
  group?: string;
  /** Position among its siblings, lower first; keys without one follow, A→Z. */
  order?: number;
  /** Only shown on this OS. */
  platform?: 'linux' | 'macos';
  /** Label and help per enum value. */
  options?: Record<string, CopyOption>;
}

export type CopyTable = Record<string, CopyEntry>;

export const SECTION_COPY: Partial<Record<SettingsSectionId, CopyTable>> = {
  general,
  window: windowCopy,
  keys,
  terminal,
  'linux-graphics': linuxGraphics,
  projects,
  sources,
  accounts,
  claude,
  editors,
  worktree,
  reviews,
  notifications,
  performance,
  tools,
};

/** Every entry by dotted settings path. */
export const COPY: CopyTable = Object.assign({}, ...Object.values(SECTION_COPY));

/** Sections whose copy is finished: every key has a label (copy.test.ts). Section tickets add theirs. */
export const COPY_DONE: readonly SettingsSectionId[] = ['worktree', 'terminal'];

export const onThisPlatform = (entry: CopyEntry | undefined): boolean =>
  !entry?.platform || entry.platform === currentPlatform();

/** Consecutive runs of `[path, …]` entries sharing a `group` heading (undefined: no heading). */
export function groupRuns<T extends readonly [string, unknown]>(
  entries: readonly T[],
): { group?: string; items: T[] }[] {
  const runs: { group?: string; items: T[] }[] = [];
  for (const e of entries) {
    const group = COPY[e[0]]?.group;
    const last = runs.at(-1);
    if (last && last.group === group) last.items.push(e);
    else runs.push({ group, items: [e] });
  }
  return runs;
}
