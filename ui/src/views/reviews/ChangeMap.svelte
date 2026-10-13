<script lang="ts">
  // The change map (#401): on a row, a bar as long as the PR (linear, cut past 2k lines) split by area,
  // "1.3k lines, 49 files" under it; in the brief, a full-width bar with each area's lines.
  import { t, tn } from '$lib/i18n/index.svelte';

  import { AREA_LABEL, AREAS, barOf, kLines, type ChangeMap } from './changemap';

  interface Props {
    map: ChangeMap;
    big?: boolean;
  }

  let { map, big = false }: Props = $props();

  const bar = $derived(big ? { width: 1, capped: false } : barOf(map.lines));
  const shown = $derived(AREAS.filter((a) => map.areas[a] > 0));
  const label = $derived(
    shown.map((a) => `${t(AREA_LABEL[a])} ${map.areas[a]}`).join(', ') +
      (bar.capped ? `. ${t('More than the 2k lines the bar shows')}` : ''),
  );
</script>

<span class="map" class:big>
  <span class="bar" class:capped={bar.capped} style:width="{bar.width * 100}%" role="img" aria-label={label}
    >{#each shown as a (a)}<span
        class={a}
        style:flex-grow={map.areas[a]}
        title="{t(AREA_LABEL[a])}: {map.areas[a]}"
      ></span>{/each}</span
  >
  {#if big}
    <span class="legend">
      {#each shown as a (a)}<span><i class={a}></i>{t(AREA_LABEL[a])} <b>{map.areas[a]}</b></span>{/each}
      {#if map.files !== null}<span>{tn('{n} file', '{n} files', map.files)}</span>{/if}
    </span>
  {:else}
    <span class="size"
      >{map.files === null
        ? t('{lines} lines', { lines: kLines(map.lines) })
        : t('{lines} lines, {n} files', { lines: kLines(map.lines), n: map.files })}</span
    >
  {/if}
</span>

<style>
  .map {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    min-width: 0;
  }

  .bar {
    display: flex;
    gap: 1px;
    min-width: 4px;
    height: 8px;
    border-radius: 2px;
    overflow: hidden;
  }

  .bar > span {
    flex: 1 1 0;
    min-width: 2px;
  }

  /* Past 2k lines the bar stops at full width with a gap near its end: a cut, not the real length. */
  .capped {
    mask: linear-gradient(
      90deg,
      #000 calc(100% - 12px),
      transparent calc(100% - 12px) calc(100% - 8px),
      #000 calc(100% - 8px)
    );
  }

  .big .bar {
    height: 14px;
    border-radius: 3px;
  }

  .code {
    background: var(--k-fg-muted);
  }

  .tests {
    background: var(--k-info);
  }

  .config {
    background: var(--k-warn);
  }

  .other {
    background: var(--k-border-strong);
  }

  .size,
  .legend {
    font-size: var(--k-font-size-xs);
    font-variant-numeric: tabular-nums;
    color: var(--k-fg-subtle);
    white-space: nowrap;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-1) var(--k-space-3);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
    white-space: normal;
  }

  .legend > span {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
  }

  .legend b {
    font-weight: 600;
    color: var(--k-fg);
  }

  i {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }
</style>
