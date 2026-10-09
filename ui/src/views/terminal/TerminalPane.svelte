<script lang="ts">
  // SCAFFOLD STUB (L2): replace with the real view. Props are frozen by `PaneProps`.
  import type { PaneProps } from '$app/registry';
  import NotImplemented from '$lib/ui/NotImplemented.svelte';

  let { content }: PaneProps<'terminal'> = $props();
</script>

<div class="stub" data-pane-kind={content.kind}>
  <NotImplemented name="TerminalPane" owner="L2" />
</div>

<style>
  .stub {
    height: 100%;
    padding: var(--k-space-3);
  }
</style>
