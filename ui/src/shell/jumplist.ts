// Jumplist (ticket #136): every focus change (Mod+J, notification click, kelta-ctl, rail, pane clicks)
// is recorded; `nav.back` / `nav.forward` walk it.

import type { PaneId, ProjectId, TabId } from '$lib/gen';
import { findPane } from '$lib/layout';
import { GLOBAL, layout, projects, type GlobalView } from '$lib/stores';

import { GLOBAL_VIEWS } from '../views/work/nav';

import { activateProject, focusPaneById, openView } from './nav';

export interface JumpLoc {
  project: ProjectId;
  tab: TabId;
  pane: PaneId;
}

export const JUMP_CAP = 100;
// shortcut: localStorage, the contract has no command for the ui_state table (like the render probe);
// move it there when one lands.
const KEY = 'kelta.jumplist.v1';

const same = (a: JumpLoc | undefined, b: JumpLoc): boolean =>
  !!a && a.project === b.project && a.tab === b.tab && a.pane === b.pane;

export class Jumplist {
  list: JumpLoc[] = [];
  at = -1;

  /** A new place after the cursor; going somewhere new after `back` drops the forward entries. */
  record(loc: JumpLoc): boolean {
    if (same(this.list[this.at], loc)) return false;
    this.list = [...this.list.slice(0, this.at + 1), loc].slice(-JUMP_CAP);
    this.at = this.list.length - 1;
    return true;
  }

  /** Moves the cursor; null at either end. */
  step(delta: 1 | -1): JumpLoc | null {
    const to = this.list[this.at + delta];
    if (!to) return null;
    this.at += delta;
    return to;
  }
}

function load(): Jumplist {
  const j = new Jumplist();
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null') as { list: JumpLoc[]; at: number } | null;
    if (saved && Array.isArray(saved.list)) {
      // Pre-#270 entries carry `view`: a global view is now its tab in the GLOBAL layout.
      j.list = saved.list
        .slice(-JUMP_CAP)
        .map((l: JumpLoc & { view?: string }) =>
          l.view ? { project: GLOBAL, tab: l.view, pane: l.view } : l,
        );
      j.at = Math.min(saved.at, j.list.length - 1);
    }
  } catch {
    // storage unavailable or corrupt: start empty
  }
  return j;
}

export const jumplist = load();

function save(): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ list: jumplist.list, at: jumplist.at }));
  } catch {
    // storage unavailable: the jumplist lives for this window only
  }
}

function recordJump(loc: JumpLoc): void {
  if (jumplist.record(loc)) save();
}

// A place counts once focus rests on it: a cross-project jump passes through the target's old
// active tab (between `activateProject` and the tab switch or `work_resume`) and must not record it.
// shortcut: a real work_resume slower than SETTLE_MS still records that stopover; pause recording
// around the jump if that shows up.
export const SETTLE_MS = 300;
let pending: JumpLoc | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Every focus change; recorded when focus stays SETTLE_MS. */
export function noteFocus(loc: JumpLoc): void {
  clearTimeout(timer);
  pending = loc;
  timer = setTimeout(flushJump, SETTLE_MS);
}

/** Records the place focus is on now, before a jump leaves it (however briefly it was there). */
export function flushJump(): void {
  clearTimeout(timer);
  if (pending) recordJump(pending);
  pending = null;
}

/** A place `nav.back` can still land on: a global view, or a pane of an open project. */
function alive(loc: JumpLoc): boolean {
  if (loc.project === GLOBAL) return GLOBAL_VIEWS.includes(loc.tab as GlobalView);
  const tab = layout.get(loc.project)?.tabs.find((t) => t.id === loc.tab);
  return !!projects.byId(loc.project)?.open && !!tab && !!findPane(tab.root, loc.pane);
}

/** Where `nav.back` lands from `here`, without moving (the context bar's `‹` tooltip). */
export function backTarget(here: JumpLoc | null): JumpLoc | null {
  const past = jumplist.list.slice(0, jumplist.at + 1);
  if (pending && !same(past.at(-1), pending)) past.push(pending);
  for (let i = past.length - 1; i >= 0; i -= 1) {
    const loc = past[i]!;
    if (!(here && same(loc, here)) && alive(loc)) return loc;
  }
  return null;
}

/** `nav.back` / `nav.forward`: the previous / next place that still exists. */
export function navStep(delta: 1 | -1): void {
  flushJump();
  const from = jumplist.at;
  for (let loc = jumplist.step(delta); loc; loc = jumplist.step(delta)) {
    if (!alive(loc)) continue;
    save();
    // GLOBAL is no project: landing on its tab shows that view.
    if (loc.project === GLOBAL) return openView(loc.tab as GlobalView);
    void activateProject(loc.project);
    focusPaneById(loc.project, loc.tab, loc.pane);
    return;
  }
  jumplist.at = from; // no live place that way: stay put, not on a dead entry
  save();
}
