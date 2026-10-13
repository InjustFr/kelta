<script lang="ts">
  // The Queue's first-run empty state (#422): the loop Kelta wants you to work in, each step checking
  // itself off from real state. The current step carries its button.
  import { dispatch } from '$lib/actions';
  import { t } from '$lib/i18n/index.svelte';
  import { ui } from '$lib/stores';
  import { Button, Icon, Kbd } from '$lib/ui';

  import { chordFor } from '../../shell/labels';
  import { openView } from '../../shell/nav';
  import StageTrack from '../work/shared/StageTrack.svelte';
  import { stage } from '../work/stage';
  import { LOOP_STEPS, loopCurrent, type LoopStepId } from './firstRun';

  let { done }: { done: Record<LoopStepId, boolean> } = $props();

  const current = $derived(loopCurrent(done));
  const track = $derived(stage(LOOP_STEPS.find((s) => s.id === current)?.at ?? 0, 'you'));
  /** The key that does the step, shown on steps 3 to 5. */
  const KEYS: Partial<Record<LoopStepId, string>> = { start: 's', review: 'enter', ship: 'p' };
  const newWork = $derived(chordFor('work.new'));
</script>

<section class="loop" data-testid="your-loop" aria-labelledby="loop-h">
  <h2 id="loop-h">{t('Your loop')}</h2>
  <div class="track"><StageTrack stage={track} named /></div>
  <ol>
    {#each LOOP_STEPS as step (step.id)}
      <li class:current={step.id === current} aria-current={step.id === current ? 'step' : undefined}>
        <Icon name={done[step.id] ? 'check' : 'circle'} size={16} />
        <span class="label">{t(step.label)}</span>
        {#if KEYS[step.id]}<Kbd chord={KEYS[step.id]!} />{/if}
        {#if step.id === current}
          <span class="actions">
            {#if step.id === 'project'}
              <Button variant="primary" onclick={() => ui.openSheet('project_new')}>{t('Add project')}</Button
              >
            {:else if step.id === 'source'}
              <Button variant="primary" onclick={() => ui.openSheet('tracker.source_picker', {})}
                >{t('Pick a board')}</Button
              >
              <Button onclick={() => void dispatch('claude.setup')}>{t('Set up with Claude')}</Button>
            {:else if step.id === 'start'}
              <Button variant="primary" onclick={() => openView('tickets')}>{t('Tickets')}</Button>
            {/if}
          </span>
        {/if}
      </li>
    {/each}
  </ol>
  {#if newWork}
    <p class="tip">
      {t('No tickets?')}
      <Kbd chord={newWork} />
      {t('starts a branch and a Claude from one sentence.')}
    </p>
  {/if}
</section>

<style>
  .loop {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    max-width: 480px;
    padding: var(--k-space-7) var(--k-space-6);
    color: var(--k-fg-muted);
  }

  h2 {
    margin: 0;
    color: var(--k-fg);
    font-size: var(--k-font-size-lg);
  }

  .track {
    margin-block: var(--k-space-2) var(--k-space-4);
  }

  ol {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--k-space-3);
    padding: var(--k-space-2) var(--k-space-3);
    border-radius: var(--k-radius-sm);
  }

  li.current {
    background: var(--k-bg-selected);
    color: var(--k-fg);
    font-weight: 600;
  }

  .actions {
    display: flex;
    gap: var(--k-space-2);
    margin-inline-start: auto;
  }

  .tip {
    margin: var(--k-space-3) 0 0;
  }
</style>
