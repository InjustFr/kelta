<script lang="ts" generics="T">
  import { untrack, type Snippet } from 'svelte';

  import { glide, fadeMove } from './motion';

  interface Props {
    items: readonly T[];
    /** Fixed row height in px; with `measure`, the height assumed for a row not rendered yet. */
    itemHeight: number;
    /** Rows take their content's height (two-line rows whose chips wrap) instead of `itemHeight`. */
    measure?: boolean;
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
    measure = false,
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

  /** Measured heights by row key (`measure` only). */
  let sizes = $state<Record<string, number>>({});
  /** tops[i] is row i's offset; tops[items.length] the total height. Null on fixed heights. */
  const tops = $derived.by(() => {
    if (!measure) return null;
    const out = [0];
    items.forEach((item, i) => out.push(out[i] + (sizes[String(key(item, i))] ?? itemHeight)));
    return out;
  });
  const top = (i: number): number => (tops ? tops[i] : i * itemHeight);
  /** Index of the row at offset `y`. */
  function at(y: number): number {
    if (!tops) return Math.floor(y / itemHeight);
    let lo = 0;
    let hi = items.length;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (tops[mid] <= y) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  const visibleCount = $derived(Math.ceil((height || itemHeight * 20) / itemHeight));
  const start = $derived(Math.max(0, at(scrollTop) - overscan));
  const end = $derived(
    Math.min(
      items.length,
      (tops ? at(scrollTop + (height || itemHeight * 20)) + 1 : at(scrollTop) + visibleCount) + overscan,
    ),
  );
  const heads = $derived(sticky ? items.flatMap((item, i) => (sticky(item) ? [i] : [])) : []);
  // The last header scrolled above the top, pushed up by the next one as it arrives. It is the row
  // itself, sticky instead of placed, so the list never holds a header twice and keeps its order.
  const pinned = $derived.by(() => {
    const h = heads.findLastIndex((i) => top(i) < scrollTop);
    if (h < 0) return null;
    const index = heads[h]!;
    const next = heads[h + 1];
    const shift = next === undefined ? 0 : Math.min(0, top(next) - (top(index + 1) - top(index)) - scrollTop);
    return { item: items[index]!, index, shift };
  });
  const slice = $derived([
    ...(pinned && pinned.index < start ? [pinned] : []),
    ...items.slice(start, end).map((item, i) => ({ item, index: start + i })),
  ]);

  let observer: ResizeObserver | undefined;
  const observed = new WeakMap<Element, string>();
  /** Observes every row, so toggling `measure` on later still sizes the rows already mounted. */
  function sized(el: HTMLElement, k: string) {
    if (typeof ResizeObserver === 'undefined') return;
    observer ??= new ResizeObserver((entries) => {
      for (const e of entries) {
        const id = observed.get(e.target);
        const h = (e.target as HTMLElement).offsetHeight;
        if (measure && id !== undefined && h > 0 && sizes[id] !== h) sizes[id] = h;
      }
      // Measuring shifts every later offset: land the followed row again until the user takes over.
      if (followed !== undefined) scrollTo(followed);
    });
    observed.set(el, k);
    observer.observe(el);
    return {
      update: (next: string) => observed.set(el, next),
      destroy: () => observer?.unobserve(el),
    };
  }

  /** When the list mounted: rows landing at their first measured height do not slide there. */
  const born = performance.now();
  /** FLIP, but not for the pinned header (it moves with the scroll) nor while the rows first measure. */
  function move(node: Element, rects: { from: DOMRect; to: DOMRect }) {
    if (node.classList.contains('pin') || performance.now() - born < 500) return { duration: 0 };
    return glide(node, rects);
  }

  /** A row arriving with the data (a section unfolding, a row added) fades in; one scrolled into view does not. */
  let scrolledAt = 0;
  function enter(node: Element) {
    return performance.now() - Math.max(born, scrolledAt) < 500 ? { duration: 0 } : fadeMove(node);
  }

  let endNotifiedAt = -1;
  $effect(() => {
    if (!onend || items.length === 0) return;
    if (end >= items.length - endThreshold && endNotifiedAt !== items.length) {
      endNotifiedAt = items.length;
      onend();
    }
  });

  function onscroll(): void {
    scrolledAt = performance.now();
    if (viewport) scrollTop = viewport.scrollTop;
  }

  /** Row the last `scrollToIndex` targeted; re-applied after each measurement, cleared by user input. */
  let followed: number | undefined;
  const unpin = () => (followed = undefined);

  function scrollTo(index: number): void {
    // Untracked: reading `tops` must not re-run the callers' effects on every row measurement.
    untrack(() => {
      if (!viewport) return;
      // Keep a row clear of the pinned header above it.
      const y = top(index) - (heads.length > 0 && !heads.includes(index) ? itemHeight : 0);
      const bottom = top(index + 1);
      if (y < viewport.scrollTop) viewport.scrollTop = y;
      else if (bottom > viewport.scrollTop + viewport.clientHeight)
        viewport.scrollTop = bottom - viewport.clientHeight;
      scrollTop = viewport.scrollTop;
    });
  }

  /** Scrolls the minimum amount so that row `index` is fully visible, and keeps it so while rows measure. */
  export function scrollToIndex(index: number): void {
    followed = index;
    scrollTo(index);
  }
</script>

<div
  class="k-vlist"
  bind:this={viewport}
  bind:clientHeight={height}
  {onscroll}
  onwheel={unpin}
  onpointerdown={unpin}
  ontouchmove={unpin}
  role="list"
  aria-label={label}
>
  <div class="spacer" style:height="{top(items.length)}px">
    {#each slice as { item, index } (key(item, index))}
      {@const pin = index === pinned?.index}
      <!-- One element for both cases: a row that changes place slides there (animate:), a row that
           leaves fades while the rest close up (#398). The pinned header is in flow at the top of the
           spacer (the rows are absolute), so sticky pins it. -->
      <div
        class={pin ? 'pin' : 'row'}
        class:measured={measure}
        role="listitem"
        style:top={pin ? undefined : `${top(index)}px`}
        style:translate={pin ? `0 ${pinned?.shift ?? 0}px` : undefined}
        style:height={measure ? undefined : `${itemHeight}px`}
        style:min-height={measure ? `${itemHeight}px` : undefined}
        use:sized={String(key(item, index))}
        animate:move
        in:enter
        out:fadeMove
      >
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

  /* The one grid track stretches to the min-height, so a `height: 100%` child still fills the row. */
  .measured {
    display: grid;
  }
</style>
