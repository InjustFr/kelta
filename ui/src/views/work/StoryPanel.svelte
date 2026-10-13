<script lang="ts">
  // The Story panel (#423): docked on the right of a work tab (`work.story`, ⌘I), the item's story in
  // stage order; the section of its current stage is open. Also the `work_item` pane, which the start
  // saga shows while the item has no session yet.
  import { t } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { PaneContent } from '$lib/gen';
  import { work } from '$lib/stores';
  import { frames } from '$lib/terminal/raf';
  import { EmptyState } from '$lib/ui';

  import { phaseNow, prOf } from './live';
  import { CLAUDE, PR, TICKET, workStage } from './stage';
  import Changes from './story/Changes.svelte';
  import Claude from './story/Claude.svelte';
  import Log from './story/Log.svelte';
  import { STORY_MAX, STORY_MIN, story } from './story/panel.svelte';
  import PullRequest from './story/PullRequest.svelte';
  import ReturnBrief from './story/ReturnBrief.svelte';
  import Ticket from './story/Ticket.svelte';

  interface Props {
    projectId: string;
    tabId: string;
    /** Docked: the tab's work item. */
    workItemId?: string;
    /** As a `work_item` pane. */
    content?: Extract<PaneContent, { kind: 'work_item' }>;
  }

  let { projectId, tabId, workItemId, content }: Props = $props();

  const docked = $derived(!content);
  const id = $derived(content?.id ?? workItemId ?? '');
  const item = $derived(work.get(id));
  const at = $derived.by(() => {
    if (!item) return CLAUDE;
    return workStage(item, phaseNow(item), prOf(item), false).at;
  });

  $effect(() => {
    if (!work.loaded) untrack(() => void work.load().catch(() => {}));
  });

  // ---- width: drag or arrow keys on the left edge ----

  let aside = $state<HTMLElement>();
  let dragging = $state(false);
  let dragX = 0;
  const dragKey = {};

  function onEdgeDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragX = story.width;
    dragging = true;
  }

  function onEdgeMove(e: PointerEvent): void {
    if (!dragging || !aside) return;
    dragX = aside.getBoundingClientRect().right - e.clientX;
    // One resize per frame: the terminals refit on every width change.
    frames.schedule(dragKey, () => story.resize(dragX, false));
  }

  function onEdgeUp(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    frames.cancel(dragKey);
    story.resize(dragX);
  }

  function onEdgeKey(e: KeyboardEvent): void {
    const step = e.shiftKey ? 64 : 16;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      story.resize(story.width + (e.key === 'ArrowLeft' ? step : -step));
    }
  }
</script>

<!-- Stays mounted while closed: leaving the tab still stamps when you left (the return brief). -->
<aside
  class="story"
  class:docked
  class:closed={docked && !story.isOpen(tabId)}
  style:width={docked ? `${story.width}px` : undefined}
  bind:this={aside}
  aria-label={t('Story')}
  data-testid="story-panel"
>
  {#if docked}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="edge"
      class:dragging
      role="separator"
      aria-label={t('Story panel width')}
      aria-orientation="vertical"
      aria-valuenow={story.width}
      aria-valuemin={STORY_MIN}
      aria-valuemax={STORY_MAX}
      tabindex="0"
      data-testid="story-edge"
      onpointerdown={onEdgeDown}
      onpointermove={onEdgeMove}
      onpointerup={onEdgeUp}
      onpointercancel={onEdgeUp}
      onkeydown={onEdgeKey}
    ></div>
  {/if}
  {#if item}
    <div class="sections">
      <!-- `id`, not `item.id`: the brief stamps `left_at` whenever its item id changes. -->
      <ReturnBrief {projectId} workItemId={id} {tabId} />
      <Ticket {item} open={at === TICKET} />
      <Claude {item} open={at === CLAUDE} />
      <Changes {item} open={item.review_due} />
      <PullRequest {item} open={at >= PR} />
      <Log {item} />
    </div>
  {:else}
    <EmptyState
      icon="git-branch"
      title={t('Work item not found')}
      body={t('It was finished and removed, or it belongs to another project.')}
    />
  {/if}
</aside>

<style>
  .story {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    background: var(--k-bg);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
  }

  .docked {
    flex: none;
    height: auto;
    margin: calc(var(--k-gap) / 2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-screen);
  }

  .closed {
    display: none;
  }

  .sections {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .edge {
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(var(--k-gutter-hit) / -2 - var(--k-gap) / 2);
    z-index: 3;
    width: var(--k-gutter-hit);
    cursor: col-resize;
    touch-action: none;
  }

  .edge::after {
    content: '';
    position: absolute;
    inset: 0 auto 0 50%;
    width: 2px;
    transform: translateX(-50%);
    transition: background var(--k-duration-fast) var(--k-ease);
  }

  .edge:hover::after,
  .edge.dragging::after,
  .edge:focus-visible::after {
    background: var(--k-accent);
  }

  .edge:focus-visible {
    outline: none;
  }
</style>
