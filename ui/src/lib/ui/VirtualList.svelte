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
    /** Group headers: the one of the group in view stays pinned at the top while scrolling. */
    sticky?: (item: T) => boolean;
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
    sticky,
  }: Props = $props();

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);

  const visibleCount = $derived(Math.ceil((height || itemHeight * 20) / itemHeight));
  const start = $derived(Math.max(0, Math.floor(scrollTop / itemHeight) - overscan));
  const end = $derived(Math.min(items.length, Math.floor(scrollTop / itemHeight) + visibleCount + overscan));
  const heads = $derived(sticky ? items.flatMap((item, i) => (sticky(item) ? [i] : [])) : []);
  // The last header scrolled above the top, pushed up by the next one as it arrives. It is the row
  // itself, sticky instead of placed, so the list never holds a header twice and keeps its order.
  const pinned = $derived.by(() => {
    const first = scrollTop / itemHeight;
    const at = heads.findLastIndex((i) => i < first);
    if (at < 0) return null;
    const next = heads[at + 1];
    const shift = next === undefined ? 0 : Math.min(0, (next - 1) * itemHeight - scrollTop);
    return { item: items[heads[at]!]!, index: heads[at]!, shift };
  });
  const slice = $derived([
    ...(pinned && pinned.index < start ? [pinned] : []),
    ...items.slice(start, end).map((item, i) => ({ item, index: start + i })),
  ]);

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
    const bottom = (index + 1) * itemHeight;
    // Keep a row clear of the pinned header above it.
    const top = index * itemHeight - (heads.length > 0 && !heads.includes(index) ? itemHeight : 0);
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
      {#if index === pinned?.index}
        <!-- In flow at the top of the spacer (the rows are absolute), so sticky pins it. -->
        <div class="pin" role="listitem" style:height="{itemHeight}px" style:translate="0 {pinned.shift}px">
          {@render row(item, index)}
        </div>
      {:else}
        <div class="row" role="listitem" style:top="{index * itemHeight}px" style:height="{itemHeight}px">
          {@render row(item, index)}
        </div>
      {/if}
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

  .pin {
    position: sticky;
    top: 0;
    z-index: 1;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
  }
</style>
