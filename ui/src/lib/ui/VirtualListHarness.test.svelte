<script lang="ts">
  import VirtualList from './VirtualList.svelte';

  let { items }: { items: string[] } = $props();
</script>

<div style="height: 300px">
  <VirtualList {items} itemHeight={28} label="rows">
    {#snippet row(item)}<span>{item}</span>{/snippet}
  </VirtualList>
</div>
