<script lang="ts">
  import type { PrLink } from '$lib/gen';
  import { Lamp } from '$lib/ui';

  import { ciGlyph } from '../work/common';
  import { prLabel, prLamp, reviewWord } from './prs';

  // PR chip (TICKETS.md T1): mono `#482` / `!57` plus one lamp for CI and review; the title spells out both.
  let { pr }: { pr: PrLink } = $props();
  const label = $derived(`${ciGlyph(pr.ci).label}, ${reviewWord(pr).toLowerCase()}`);
</script>

<span class="pr" data-testid="pr-chip" title={`${pr.title || pr.repo} (${label})`}
  ><span class="k-mono">{prLabel(pr)}</span><Lamp level={prLamp(pr)} title={label} /></span
>

<style>
  .pr {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }
</style>
