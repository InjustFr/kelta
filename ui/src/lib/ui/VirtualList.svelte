<script lang="ts" generics="T">
  import { untrack, type Snippet } from 'svelte';

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
  const slice = $derived(items.slice(start, end).map((item, i) => ({ item, index: start + i })));

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
      if (pinned !== undefined) scrollTo(pinned);
    });
    observed.set(el, k);
    observer.observe(el);
    return {
      update: (next: string) => observed.set(el, next),
      destroy: () => observer?.unobserve(el),
    };
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
    if (viewport) scrollTop = viewport.scrollTop;
  }

  /** Row the last `scrollToIndex` targeted; re-applied after each measurement, cleared by user input. */
  let pinned: number | undefined;
  const unpin = () => (pinned = undefined);

  function scrollTo(index: number): void {
    // Untracked: reading `tops` must not re-run the callers' effects on every row measurement.
    untrack(() => {
      if (!viewport) return;
      const y = top(index);
      const bottom = top(index + 1);
      if (y < viewport.scrollTop) viewport.scrollTop = y;
      else if (bottom > viewport.scrollTop + viewport.clientHeight)
        viewport.scrollTop = bottom - viewport.clientHeight;
      scrollTop = viewport.scrollTop;
    });
  }

  /** Scrolls the minimum amount so that row `index` is fully visible, and keeps it so while rows measure. */
  export function scrollToIndex(index: number): void {
    pinned = index;
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
      <div
        class="row"
        class:measured={measure}
        role="listitem"
        style:top="{top(index)}px"
        style:height={measure ? undefined : `${itemHeight}px`}
        style:min-height={measure ? `${itemHeight}px` : undefined}
        use:sized={String(key(item, index))}
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
