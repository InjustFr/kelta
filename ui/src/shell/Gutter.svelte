<script lang="ts">
  import { frames } from '$lib/terminal/raf';

  import type { Gutter } from './geometry';

  interface Props {
    gutter: Gutter;
    /** Pixel size of the pane area (for converting drags into ratio deltas). */
    areaWidth: number;
    areaHeight: number;
    /** Ratio of the first neighbour (aria-valuenow). */
    value: number;
    onresize: (delta: number) => void;
  }

  let { gutter, areaWidth, areaHeight, value, onresize }: Props = $props();

  const row = $derived(gutter.dir === 'row');
  const key = {};
  let dragging = $state(false);
  let last = 0;
  let pendingPx = 0;

  function span(): number {
    return (row ? gutter.split.w * areaWidth : gutter.split.h * areaHeight) || 1;
  }

  function flush(): void {
    const px = pendingPx;
    pendingPx = 0;
    if (px !== 0) onresize(px / span());
  }

  function onpointerdown(e: PointerEvent): void {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    last = row ? e.clientX : e.clientY;
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging) return;
    const pos = row ? e.clientX : e.clientY;
    pendingPx += pos - last;
    last = pos;
    // Coalesce pointer moves to one layout update per frame.
    frames.schedule(key, flush);
  }

  function onpointerup(e: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    frames.cancel(key);
    flush();
  }

  function onkeydown(e: KeyboardEvent): void {
    const step = e.shiftKey ? 0.05 : 0.02;
    const keys = row ? { ArrowLeft: -step, ArrowRight: step } : { ArrowUp: -step, ArrowDown: step };
    const delta = (keys as Record<string, number | undefined>)[e.key];
    if (delta === undefined) return;
    e.preventDefault();
    onresize(delta);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="gutter"
  class:row
  class:dragging
  role="separator"
  aria-orientation={row ? 'vertical' : 'horizontal'}
  aria-valuenow={Math.round(value * 100)}
  aria-valuemin={5}
  aria-valuemax={95}
  tabindex="0"
  data-testid="gutter"
  style:left="{gutter.line.x * 100}%"
  style:top="{gutter.line.y * 100}%"
  style:width={row ? undefined : `${gutter.line.w * 100}%`}
  style:height={row ? `${gutter.line.h * 100}%` : undefined}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  {onkeydown}
></div>

<style>
  .gutter {
    position: absolute;
    z-index: 2;
    touch-action: none;
  }

  /* Hit area centred on the split line, which is the middle of the housing gap. */
  .gutter.row {
    width: var(--k-gutter-hit);
    margin-left: calc(var(--k-gutter-hit) / -2);
    cursor: col-resize;
  }

  .gutter:not(.row) {
    height: var(--k-gutter-hit);
    margin-top: calc(var(--k-gutter-hit) / -2);
    cursor: row-resize;
  }

  /* Always-visible grip: says "this edge moves" without hovering. */
  .gutter::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 4px;
    height: 28px;
    border-radius: 2px;
    background: var(--k-border-strong);
    opacity: 0.7;
    transform: translate(-50%, -50%);
    transition:
      background var(--k-duration) ease-out,
      opacity var(--k-duration) ease-out;
  }

  .gutter:not(.row)::after {
    width: 28px;
    height: 4px;
  }

  .gutter:hover::after,
  .gutter.dragging::after,
  .gutter:focus-visible::after {
    background: var(--k-accent);
    opacity: 1;
  }

  .gutter:focus-visible {
    outline-offset: -2px;
    border-radius: var(--k-radius-sm);
  }
</style>
