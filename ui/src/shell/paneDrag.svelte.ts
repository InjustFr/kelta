// The pane dragged by its header (ticket #341). Set on dragstart, cleared on drop, dragend or Escape;
// the workspace and the rail drop it with `drop()`.

import type { PaneId, ProjectId } from '$lib/gen';
import { movePane, type DropZone, type PaneDrop, type PaneLocation, type Rect } from '$lib/layout';
import { layout } from '$lib/stores';

class PaneDragStore {
  current = $state<(PaneLocation & { projectId: ProjectId }) | null>(null);
  /** The workspace drop indicator: target pane, zone and covered area (unit square). */
  over = $state<{ paneId: PaneId; zone: DropZone; area: Rect } | null>(null);

  start(projectId: ProjectId, from: PaneLocation): void {
    this.current = { projectId, ...from };
    this.over = null;
  }

  end(): void {
    this.current = null;
    this.over = null;
  }

  /** Moves the dragged pane; a pane from another project stays where it is. */
  drop(projectId: ProjectId, to: PaneDrop): void {
    const from = this.current;
    this.end();
    if (from?.projectId === projectId) layout.update(projectId, (l) => movePane(l, from, to));
  }
}

export const paneDrag = new PaneDragStore();

/**
 * A drag that ended outside the window (#342: dragged out, the pane or tab moves to a new window).
 * shortcut: trusts dragend's client coordinates; a webview reporting (0, 0) there just never detaches.
 */
export function droppedOutside(e: DragEvent): boolean {
  return e.clientX < 0 || e.clientY < 0 || e.clientX > window.innerWidth || e.clientY > window.innerHeight;
}
