<script lang="ts">
  import VirtualList from './VirtualList.svelte';

  let { items, sticky }: { items: string[]; sticky?: (item: string) => boolean } = $props();
  let list = $state<VirtualList<string>>();
  export const scrollToIndex = (i: number) => list?.scrollToIndex(i);
</script>

<div style="height: 300px">
  <VirtualList bind:this={list} {items} {sticky} itemHeight={28} label="rows">
    {#snippet row(item)}<span>{item}</span>{/snippet}
  </VirtualList>
</div>
