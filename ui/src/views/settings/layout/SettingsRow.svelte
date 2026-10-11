<script lang="ts">
  // The one settings row (DESIGN §6.12): label, markers and help on the left, the control in its own
  // right-hand column; one column when the row is narrow. Field and every custom panel use it.
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';

  interface Props extends HTMLAttributes<HTMLDivElement> {
    label?: string;
    /** id of the control the label names (a plain text label otherwise). */
    for?: string;
    /** Badges and reset, right of the label. */
    markers?: Snippet;
    control: Snippet;
    /** The control column holds buttons: right-aligned. */
    actions?: boolean;
    readonly?: boolean;
    /** Help, warnings and errors under the label. */
    children?: Snippet;
  }

  let {
    label,
    for: forId,
    markers,
    control,
    actions = false,
    readonly = false,
    children,
    ...rest
  }: Props = $props();
</script>

<div class="s-row" class:readonly {...rest}>
  <div class="head">
    {#if label && forId}<label for={forId}>{label}</label>{:else if label}<span>{label}</span>{/if}
    {#if markers}<span class="markers">{@render markers()}</span>{/if}
  </div>
  <div class="control" class:actions>{@render control()}</div>
  {#if children}<div class="help">{@render children()}</div>{/if}
</div>

<style>
  .s-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(200px, 340px);
    gap: var(--k-space-1) var(--k-space-6);
    align-items: start;
    padding: var(--k-space-3) 0;
    border-bottom: 1px solid var(--k-border);
    container-type: inline-size;
  }

  .head {
    grid-column: 1;
    grid-row: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--k-space-3);
    min-height: var(--k-control-height);
  }

  .markers {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    margin-left: auto;
  }

  .control {
    grid-column: 2;
    grid-row: 1 / span 2;
    min-width: 0;
  }

  .control.actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: var(--k-space-2);
    min-height: var(--k-control-height);
  }

  .help {
    grid-column: 1;
    grid-row: 2;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .help :global(p) {
    margin: 0;
  }

  /* A toggle carries its own label: it spans the row with its text left and the switch in the
     control column, and the head (markers) sits over column 1 so badges keep one slot per row. */
  .s-row[data-kind='bool'] > .control {
    grid-column: 1 / -1;
    grid-row: 1;
  }

  .s-row[data-kind='bool'] > .control :global(.k-toggle) {
    display: flex;
    flex-direction: row-reverse;
    justify-content: space-between;
    min-height: var(--k-control-height);
  }

  .s-row[data-kind='bool'] > .head {
    position: relative;
    pointer-events: none;
  }

  .s-row[data-kind='bool'] > .head > .markers {
    pointer-events: auto;
  }

  /* Narrow (a split pane): one column, label then control then help. The row is its own container,
     so its two tracks stay; every child spans both instead. A toggle's badges move under it. */
  @container (max-width: 560px) {
    .head,
    .control,
    .help,
    .s-row[data-kind='bool'] > .control {
      grid-column: 1 / -1;
      grid-row: auto;
    }

    .control.actions {
      justify-content: flex-start;
    }

    .s-row[data-kind='bool'] > .head {
      grid-row: 2;
      min-height: 0;
    }

    .s-row[data-kind='bool'] .markers {
      margin-left: 0;
    }
  }

  .readonly {
    opacity: 0.7;
  }
</style>
