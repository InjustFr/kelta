// Opening panes from the work views: reuses an existing pane showing the same thing, else opens
// one per the requested placement.

import type { PaneContent, Placement, ProjectId, WorkItemId } from '$lib/gen';
import { activateTab, contentEquals, findContent, focusPane } from '$lib/layout';
import { layout } from '$lib/stores';

export interface OpenOptions {
  placement?: Placement;
  title?: string | null;
  workItemId?: WorkItemId | null;
  /** Reuse any pane matching this predicate instead of strict content equality. */
  match?: (content: PaneContent) => boolean;
}

/** Focuses the existing pane for `content` or opens a new one. Resolves false when no layout. */
export async function openContent(
  projectId: ProjectId,
  content: PaneContent,
  opts: OpenOptions = {},
): Promise<boolean> {
  try {
    await layout.ensure(projectId);
  } catch {
    return false;
  }
  const current = layout.get(projectId);
  if (!current) return false;
  const match = opts.match ?? ((c: PaneContent) => contentEquals(c, content));
  const hit = findContent(current, match);
  if (hit) {
    layout.update(projectId, (l) => activateTab(focusPane(l, hit.tabId, hit.paneId), hit.tabId));
    return true;
  }
  layout.open(projectId, {
    content,
    placement: opts.placement ?? 'split_right',
    focus: true,
    tab_title: opts.title ?? null,
    work_item_id: opts.workItemId ?? null,
  });
  return true;
}
