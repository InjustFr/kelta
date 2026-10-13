<script lang="ts">
  // Kelta's start steps: folded, unless one failed (then Retry and Skip step sit on it).
  import { t } from '$lib/i18n/index.svelte';

  import type { WorkItem } from '$lib/gen';
  import { Button, Icon } from '$lib/ui';

  import { runWorkAction } from '../actions';
  import { stepLabel } from '../common';
  import Section from './Section.svelte';

  let { item }: { item: WorkItem } = $props();

  const failed = $derived(item.steps.find((s) => s.status === 'failed') ?? null);
  const summary = $derived(
    failed
      ? t('{step} failed', { step: stepLabel(failed.step) })
      : t('{done} of {n} steps', {
          done: item.steps.filter((s) => s.status === 'done' || s.status === 'skipped').length,
          n: item.steps.length,
        }),
  );
  let busy = $state<'retry' | 'skip_step' | null>(null);

  async function run(id: 'retry' | 'skip_step'): Promise<void> {
    busy = id;
    try {
      await runWorkAction(id, item);
    } finally {
      busy = null;
    }
  }
</script>

<Section title={t('Log')} {summary} open={failed !== null} testid="story-log">
  <ol class="steps" aria-label={t('Steps')}>
    {#each item.steps as s (s.step)}
      <li data-status={s.status}>
        {#if s.status === 'done'}
          <Icon name="circle-check" size={14} />
        {:else if s.status === 'failed'}
          <Icon name="circle-x" size={14} />
        {:else if s.status === 'skipped'}
          <Icon name="minus" size={14} />
        {:else}
          <Icon name="circle" size={14} />
        {/if}
        <span>{stepLabel(s.step)}</span>
        {#if s.detail}<small>{s.detail}</small>{/if}
      </li>
      {#if s.status === 'failed' && item.state.kind === 'failed'}
        <li class="actions">
          <Button size="sm" loading={busy === 'retry'} onclick={() => void run('retry')}>{t('Retry')}</Button>
          <Button
            size="sm"
            variant="ghost"
            loading={busy === 'skip_step'}
            onclick={() => void run('skip_step')}>{t('Skip step')}</Button
          >
        </li>
      {/if}
    {/each}
  </ol>
</Section>

<style>
  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
  }

  .actions {
    padding-left: calc(14px + var(--k-space-2));
  }

  [data-status='done'] :global(.k-icon) {
    color: var(--k-ok);
  }

  [data-status='failed'] {
    color: var(--k-danger);
  }

  [data-status='pending'] {
    color: var(--k-fg-subtle);
  }

  small {
    color: var(--k-fg-subtle);
    overflow-wrap: anywhere;
  }
</style>
