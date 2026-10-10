// Jumplist (ticket #136): every focus change (Mod+J, notification click, kelta-ctl, rail, pane clicks)
// is recorded; `nav.back` / `nav.forward` walk it.

import type { PaneId, ProjectId, TabId } from '$lib/gen';
import { findPane } from '$lib/layout';
import { layout, projects } from '$lib/stores';

import { activateProject, focusPaneById } from './nav';

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
      j.list = saved.list.slice(-JUMP_CAP);
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

/** `nav.back` / `nav.forward`: the previous / next place that still exists. */
export function navStep(delta: 1 | -1): void {
  flushJump();
  for (let loc = jumplist.step(delta); loc; loc = jumplist.step(delta)) {
    const tab = layout.get(loc.project)?.tabs.find((t) => t.id === loc.tab);
    if (!projects.byId(loc.project)?.open || !tab || !findPane(tab.root, loc.pane)) continue;
    save();
    void activateProject(loc.project);
    focusPaneById(loc.project, loc.tab, loc.pane);
    return;
  }
  save();
}
