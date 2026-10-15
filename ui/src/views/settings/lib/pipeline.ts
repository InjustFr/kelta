import { msg } from '$lib/i18n/index.svelte';
import type { PipelineEvent, StepDef, TrackerBinding, TransitionTarget, WorkSettings } from '$lib/gen';

// Mirror of kelta_proto::settings::effective_pipeline (settings.rs): keep the two in step.
/** The steps of a board's tickets: its `pipeline`, else `work.pipeline`, else the flow `status_map`
 *  and `work.on_*` describe (a `status_map` key wins over its `on_*` target). */
export function effectivePipeline(tracker: Pick<TrackerBinding, 'pipeline' | 'status_map'>, work: WorkSettings): StepDef[] {
  if (tracker.pipeline?.length) return tracker.pipeline;
  if (work.pipeline?.length) return work.pipeline;
  const map = tracker.status_map;
  const step = (
    id: string,
    label: string,
    enter: PipelineEvent[],
    transition_to: TransitionTarget | null = null,
    comment: string | null = null,
  ): StepDef => ({ id, label, enter, transition_to, comment });
  return [
    step(
      'implementing',
      msg('Implementing'),
      ['work_started'],
      map.start ?? work.on_start.transition_to,
      work.on_start.comment,
    ),
    step('pr', msg('PR'), ['pr_opened'], map.review ?? work.on_pr.transition_to, work.on_pr.comment),
    step('review', msg('Review'), ['pr_ready', 'review_requested']),
    step('merge', msg('Merge'), ['approved']),
    step('done', msg('Done'), ['merged'], map.done ?? work.on_merge.transition_to),
  ];
}
