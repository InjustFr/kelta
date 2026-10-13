<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';

  // A list that is fetching (first load or refresh) says so on the edge under its toolbar (#398).
  // Zero height in the flow: the bar overlays the list's first 2px, nothing shifts.
  let { active, label }: { active: boolean; label?: string } = $props();
</script>

<div class="k-loadbar">
  {#if active}<div class="bar" role="progressbar" aria-label={label ?? t('Loading')}></div>{/if}
</div>

<style>
  .k-loadbar {
    position: relative;
    z-index: 2;
    height: 0;
  }

  /* Mounted only while the load is in flight, after --k-delay-show: the loop stops with it. */
  .bar {
    position: absolute;
    inset: 0 0 auto;
    height: 2px;
    overflow: hidden;
    pointer-events: none;
    animation: k-show var(--k-duration) var(--k-ease) var(--k-delay-show) backwards;
  }

  /* Still under reduced motion: a third of the bar, lit. */
  .bar::after {
    content: '';
    position: absolute;
    inset: 0 auto 0 0;
    width: 34%;
    background: var(--k-accent);
    animation: k-load 1.2s var(--k-ease) infinite;
  }

  @keyframes k-load {
    from {
      translate: -100% 0;
    }
    to {
      translate: 300% 0;
    }
  }
</style>
