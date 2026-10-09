<script lang="ts">
  // SCAFFOLD STUB (L4): settings section. Props are frozen by `SettingsSectionProps`.
  import type { SettingsSectionProps } from '$app/registry';
  import NotImplemented from '$lib/ui/NotImplemented.svelte';

  let { layer }: SettingsSectionProps = $props();
</script>

<section data-layer={layer}>
  <NotImplemented name="General settings" owner="L4" />
</section>
