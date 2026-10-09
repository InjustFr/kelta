<script lang="ts">
  // SCAFFOLD STUB (L8): replace with the real sheet. Props are frozen by `SheetProps`.
  import type { SheetProps } from '$app/registry';
  import NotImplemented from '$lib/ui/NotImplemented.svelte';
  import Sheet from '$lib/ui/Sheet.svelte';

  let { onclose }: SheetProps = $props();
</script>

<Sheet title="Open tool" {onclose}>
  <NotImplemented name="ToolPicker" owner="L8" />
</Sheet>
