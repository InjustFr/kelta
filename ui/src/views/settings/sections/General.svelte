<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';

  import SectionForm from '../SectionForm.svelte';
  import ThemePicker from '../ThemePicker.svelte';
  import Updates from '../Updates.svelte';

  let props: SettingsSectionProps = $props();
</script>

<SectionForm sectionId="general" {...props} skip={['app.dark_theme', 'app.light_theme']}>
  {#snippet before()}
    {#if props.layer === 'global'}
      <Updates />
      <ThemePicker />
    {:else}
      <p class="note" data-testid="theme-note">{t('Themes are chosen in global settings.')}</p>
    {/if}
  {/snippet}
</SectionForm>

<style>
  .note {
    margin: 0 0 var(--k-space-3);
    color: var(--k-fg-muted);
  }
</style>
