<script lang="ts">
  // Settings → Work items (id `worktree`): stage links to the copy groups, and where ticket moves live now.
  import { dispatch } from '$lib/actions';
  import { t } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';
  import { settings } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';

  import { STAGES } from '../copy/worktree';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  const jump = (group: string) =>
    document.querySelector(`[data-group="${CSS.escape(group)}"]`)?.scrollIntoView?.({ block: 'start' });
  const editSteps = () =>
    void dispatch('settings.open', {
      section: 'sources',
      source_id: settings.value()?.sources[0]?.id,
      anchor: 'steps',
    });
</script>

<SectionForm sectionId="worktree" {...props}>
  {#snippet before()}
    {#if props.layer !== 'repo'}
      <nav class="stages" aria-label={t('Jump to a stage')} data-testid="work-stages">
        {#each STAGES as stage (stage)}
          <Button size="sm" variant="ghost" onclick={() => jump(stage)}>{t(stage)}</Button>
        {/each}
      </nav>
      <p class="note" data-testid="steps-note">
        <span
          >{t(
            'Where the ticket moves and what gets commented at each stage come from the steps of its board.',
          )}</span
        >
        <Button size="sm" variant="ghost" onclick={editSteps}>{t('Edit steps in Sources')}</Button>
      </p>
    {/if}
  {/snippet}
</SectionForm>

<style>
  .stages {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2);
  }

  .note {
    margin: var(--k-space-3) 0 0;
    color: var(--k-fg-muted);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
    font-size: var(--k-font-size-sm);
  }
</style>
