// Attention per project + global needs-input count (rail dots, Inbox badge, dock badge mirror).

import type { Attention, ProjectId, ProjectInfo, UiEvent } from '$lib/gen';

import {
  attentionFromProjects,
  reduceAttention,
  type AttentionState,
  type ProjectAttention,
} from './reducers';

export class AttentionStore {
  state = $state<AttentionState>({ byProject: {}, totalNeedsInput: 0 });

  get totalNeedsInput(): number {
    return this.state.totalNeedsInput;
  }

  forProject(id: ProjectId): ProjectAttention {
    return this.state.byProject[id] ?? { level: 'none', needs_input_count: 0 };
  }

  level(id: ProjectId): Attention {
    return this.forProject(id).level;
  }

  seed(projects: readonly ProjectInfo[]): void {
    this.state = attentionFromProjects(projects);
  }

  apply(ev: UiEvent): void {
    const next = reduceAttention(this.state, ev);
    if (next !== this.state) this.state = next;
  }
}
