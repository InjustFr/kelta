<script lang="ts">
  import VirtualList from './VirtualList.svelte';

  let { items, measure = false }: { items: string[]; measure?: boolean } = $props();
</script>

<div style="height: 300px">
  <VirtualList {items} itemHeight={28} {measure} key={(item) => item} label="rows">
    {#snippet row(item)}<span>{item}</span>{/snippet}
  </VirtualList>
</div>
