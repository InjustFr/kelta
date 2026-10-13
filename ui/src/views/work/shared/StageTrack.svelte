<script lang="ts">
  // The stage track (#399): Ticket to Deploy. The line runs to the furthest stage reached, the mark sits
  // where the item is now and its shape says whose move it is. Now, Tickets and Reviews rows share it.
  import { msg, t } from '$lib/i18n/index.svelte';

  import { STAGES, type Mark, type Stage } from '../stage';

  interface Props {
    stage: Stage;
    /** The selected row names its stage under the mark. */
    named?: boolean;
  }

  let { stage, named = false }: Props = $props();

  const WHOSE: Record<Mark, string> = {
    you: msg('your move'),
    wait: msg('waiting on others'),
    broken: msg('broken'),
    done: msg('done'),
  };
  const last = STAGES.length - 1;
  const pct = (i: number): string => `${(i / last) * 100}%`;
  const label = $derived(
    stage.unconfirmed
      ? t('At {stage} per the tracker; no branch or PR found for the stages before', {
          stage: t(STAGES[stage.at]!),
        })
      : t('At {stage}, {whose}; furthest stage reached: {reached}', {
          stage: t(STAGES[stage.at]!),
          whose: t(WHOSE[stage.mark]),
          reached: t(STAGES[stage.reached]!),
        }),
  );
</script>

<span class="track" class:named role="img" aria-label={label} title={label} data-mark={stage.mark}>
  <span class="line" class:unconfirmed={stage.unconfirmed} style:width={pct(stage.reached)}></span>
  {#each STAGES as _, i (i)}<i class:past={!stage.unconfirmed && i <= stage.reached} style:left={pct(i)}
    ></i>{/each}
  <span class="mark {stage.mark}" style:left={pct(stage.at)}></span>
  {#if named}<span
      class="here"
      class:first={stage.at === 0}
      class:last={stage.at === last}
      style:left={pct(stage.at)}
      >{stage.unconfirmed
        ? t('{stage}, per the tracker', { stage: t(STAGES[stage.at]!) })
        : t(STAGES[stage.at]!)}</span
    >{/if}
</span>

<style>
  .track {
    position: relative;
    display: block;
    height: 18px;
    margin-inline: 8px;
    /* The ring and the halo take the row's own background (hover, selected) through this. */
    --halo: var(--track-bg, var(--k-bg));
  }

  .named {
    margin-bottom: 14px;
  }

  .track::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    border-top: 1px dashed var(--k-border-strong);
  }

  .line {
    position: absolute;
    left: 0;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    border-radius: 1px;
    background: var(--k-fg-subtle);
  }

  /* Stages Kelta did not see (#400): dotted, not drawn. */
  .line.unconfirmed {
    height: 0;
    margin-top: -1px;
    border-top: 2px dotted var(--k-fg-subtle);
    border-radius: 0;
    background: none;
  }

  i {
    position: absolute;
    top: 50%;
    width: 5px;
    height: 5px;
    margin: -2.5px 0 0 -2.5px;
    border-radius: 50%;
    background: var(--k-border-strong);
  }

  i.past {
    background: var(--k-fg-subtle);
  }

  .mark {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
    border-radius: 50%;
  }

  .you {
    background: var(--k-accent);
    box-shadow: 0 0 0 3px var(--halo);
  }

  .wait {
    border: 2px solid var(--k-fg-subtle);
    background: var(--halo);
  }

  .broken {
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border-radius: 2px;
    rotate: 45deg;
    background: var(--k-danger);
    box-shadow: 0 0 0 3px var(--halo);
  }

  .done {
    width: 8px;
    height: 8px;
    margin: -4px 0 0 -4px;
    background: var(--k-fg-subtle);
  }

  .here {
    position: absolute;
    top: 19px;
    translate: -50% 0;
    font-size: var(--k-font-size-xs);
    line-height: 1;
    white-space: nowrap;
    color: var(--k-fg-muted);
  }

  .here.first {
    translate: none;
  }

  .here.last {
    translate: -100% 0;
  }
</style>
