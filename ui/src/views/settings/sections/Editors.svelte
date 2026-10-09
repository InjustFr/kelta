<script lang="ts">
  import type { SettingsSectionProps } from '$app/registry';

  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();
</script>

<SectionForm sectionId="editors" {...props} />
