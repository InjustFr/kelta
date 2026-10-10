<script lang="ts">
  import type { Status } from '$lib/gen';

  import { categoryBar } from './group';

  // Flat status chip (DESIGN §6.11): the native name, the category as a 2px left bar.
  let { status }: { status: Status } = $props();
</script>

<span class="chip" style:--bar={categoryBar(status.category)} title={status.name}>{status.name}</span>

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    flex: none;
    max-width: 140px;
    height: 18px;
    padding: 0 5px 0 7px;
    overflow: hidden;
    border-radius: var(--k-radius-sm);
    box-shadow: inset 2px 0 0 var(--bar);
    background: var(--k-bezel-raised);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    line-height: 1;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
