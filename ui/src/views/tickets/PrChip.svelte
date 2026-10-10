<script lang="ts">
  import type { PrLink } from '$lib/gen';
  import { Lamp } from '$lib/ui';

  import { ciLamp, prLabel } from './prs';

  // PR chip (TICKETS.md T1): mono `#482` / `!57` plus the CI lamp shape; the title spells it out.
  let { pr }: { pr: PrLink } = $props();
</script>

<span class="pr" data-testid="pr-chip" title={`${pr.title || pr.repo} (CI ${pr.ci})`}
  ><span class="k-mono">{prLabel(pr)}</span><Lamp level={ciLamp(pr.ci)} title={`CI ${pr.ci}`} /></span
>

<style>
  .pr {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }
</style>
