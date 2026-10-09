<script lang="ts">
  // SCAFFOLD STUB (L9): replace with the real header. Props are frozen by `TabHeaderProps`.
  import type { TabHeaderProps } from '$app/registry';
  import NotImplemented from '$lib/ui/NotImplemented.svelte';

  let { workItemId }: TabHeaderProps = $props();
</script>

<div class="stub" data-work-item={workItemId}>
  <NotImplemented name="WorkItemHeader" owner="L9" />
</div>

<style>
  .stub {
    height: 40px;
  }
</style>
