<script lang="ts">
  import type { Status } from '$lib/gen';

  import { categoryBar } from './group';

  // Status (DESIGN §6.11): a dot in the category colour, then the tracker's own status name.
  let { status }: { status: Status } = $props();
</script>

<span class="chip" title={status.name}
  ><span class="dot" style:background={categoryBar(status.category)}></span>{status.name}</span
>

<style>
  .chip {
    display: inline-block;
    max-width: 140px;
    overflow: hidden;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* Centred on the cap height (0.7em), where the status name's capitals sit, at any size. */
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-right: 6px;
    border-radius: 50%;
    vertical-align: calc(0.35em - 3.5px);
  }
</style>
