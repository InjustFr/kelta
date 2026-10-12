<script lang="ts">
  import VirtualList from './VirtualList.svelte';

  let {
    items,
    measure = false,
    follow,
    sticky,
  }: { items: string[]; measure?: boolean; follow?: number; sticky?: (item: string) => boolean } = $props();
  let list = $state<VirtualList<string>>();
  export const scrollToIndex = (i: number) => list?.scrollToIndex(i);
  // Mirrors the panes: scroll to the selection whenever it changes.
  $effect(() => {
    if (follow !== undefined) list?.scrollToIndex(follow);
  });
</script>

<div style="height: 300px">
  <VirtualList bind:this={list} {items} {sticky} {measure} itemHeight={28} key={(item) => item} label="rows">
    {#snippet row(item)}<span>{item}</span>{/snippet}
  </VirtualList>
</div>
