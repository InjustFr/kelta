<script lang="ts" generics="T">
  import type { Snippet } from 'svelte';

  interface Props {
    items: readonly T[];
    /** Fixed row height in px. */
    itemHeight: number;
    /** Extra rows rendered above/below the viewport. */
    overscan?: number;
    /** Stable key per item (defaults to the index). */
    key?: (item: T, index: number) => string | number;
    row: Snippet<[T, number]>;
    /** Accessible label of the list. */
    label?: string;
    /** Called when the user scrolls within `threshold` rows of the end (load more). */
    onend?: () => void;
    endThreshold?: number;
  }

  let {
    items,
    itemHeight,
    overscan = 6,
    key = (_item: T, index: number) => index,
    row,
    label,
    onend,
    endThreshold = 10,
  }: Props = $props();

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);

  const visibleCount = $derived(Math.ceil((height || itemHeight * 20) / itemHeight));
  const start = $derived(Math.max(0, Math.floor(scrollTop / itemHeight) - overscan));
  const end = $derived(Math.min(items.length, Math.floor(scrollTop / itemHeight) + visibleCount + overscan));
  const slice = $derived(items.slice(start, end).map((item, i) => ({ item, index: start + i })));

  let endNotifiedAt = -1;
  $effect(() => {
    if (!onend || items.length === 0) return;
    if (end >= items.length - endThreshold && endNotifiedAt !== items.length) {
      endNotifiedAt = items.length;
      onend();
    }
  });

  function onscroll(): void {
    if (viewport) scrollTop = viewport.scrollTop;
  }

  /** Scrolls the minimum amount so that row `index` is fully visible. */
  export function scrollToIndex(index: number): void {
    if (!viewport) return;
    const top = index * itemHeight;
    const bottom = top + itemHeight;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (bottom > viewport.scrollTop + viewport.clientHeight)
      viewport.scrollTop = bottom - viewport.clientHeight;
    scrollTop = viewport.scrollTop;
  }
</script>

<div
  class="k-vlist"
  bind:this={viewport}
  bind:clientHeight={height}
  {onscroll}
  role="list"
  aria-label={label}
>
  <div class="spacer" style:height="{items.length * itemHeight}px">
    {#each slice as { item, index } (key(item, index))}
      <div class="row" role="listitem" style:top="{index * itemHeight}px" style:height="{itemHeight}px">
        {@render row(item, index)}
      </div>
    {/each}
  </div>
</div>

<style>
  .k-vlist {
    position: relative;
    height: 100%;
    overflow-y: auto;
    contain: strict;
  }

  .spacer {
    position: relative;
    width: 100%;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
  }
</style>
