<script lang="ts">
  import VirtualList from './VirtualList.svelte';

  let { items, measure = false, follow }: { items: string[]; measure?: boolean; follow?: number } = $props();
  let vlist = $state<ReturnType<typeof VirtualList<string>>>();
  // Mirrors the panes: scroll to the selection whenever it changes.
  $effect(() => {
    if (follow !== undefined) vlist?.scrollToIndex(follow);
  });
</script>

<div style="height: 300px">
  <VirtualList bind:this={vlist} {items} itemHeight={28} {measure} key={(item) => item} label="rows">
    {#snippet row(item)}<span>{item}</span>{/snippet}
  </VirtualList>
</div>
