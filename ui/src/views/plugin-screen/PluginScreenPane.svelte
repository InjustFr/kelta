<script lang="ts">
  // SCAFFOLD STUB (L8): replace with the real view. Props are frozen by `PaneProps`.
  import type { PaneProps } from '$app/registry';
  import NotImplemented from '$lib/ui/NotImplemented.svelte';

  let { content }: PaneProps<'plugin_screen'> = $props();
</script>

<div class="stub" data-pane-kind={content.kind}>
  <NotImplemented name="PluginScreenPane" owner="L8" />
</div>

<style>
  .stub {
    height: 100%;
    padding: var(--k-space-3);
  }
</style>
