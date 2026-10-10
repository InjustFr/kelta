<script lang="ts">
  import type { LampLevel } from '$lib/stores/reducers';

  // Lamp (DESIGN §6.1): each state has its own silhouette so colour is never the only signal.
  interface Props {
    level: LampLevel;
    title?: string;
    /** Status word shown beside the lamp (use `attentionLabel`). The lamp is then decorative. */
    label?: string;
  }

  let { level, title, label }: Props = $props();

  const LABELS: Record<LampLevel, string> = {
    none: '',
    activity: 'activity',
    done: 'done',
    working: 'working',
    parked: 'parked',
    error: 'error',
    needs_input: 'needs input',
  };
</script>

{#if level !== 'none'}
  {#if label}
    <span class="lamp-wrap" {title}>
      <span class="lamp {level}" aria-hidden="true" data-attention={level}></span>
      <span class="lamp-label">{label}</span>
    </span>
  {:else}
    <span
      class="lamp {level}"
      role="img"
      aria-label={title ?? LABELS[level]}
      title={title ?? LABELS[level]}
      data-attention={level}
    ></span>
  {/if}
{/if}

<style>
  .lamp-wrap {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .lamp-label {
    color: var(--k-fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .lamp {
    display: inline-block;
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--c);
  }

  .needs_input {
    --c: var(--k-lamp-needs-input);
    box-shadow: 0 0 0 2px color-mix(in oklab, var(--c) 35%, transparent);
  }

  .working {
    --c: var(--k-lamp-working);
    background: transparent;
    border: 1.5px solid var(--c);
  }

  /* Parked (#142): hollow like working, but quiet. */
  .parked {
    --c: var(--k-fg-muted);
    background: transparent;
    border: 1.5px solid var(--c);
  }

  .error {
    --c: var(--k-lamp-error);
    width: 9px;
    height: 9px;
    border-radius: 0;
    transform: rotate(45deg);
  }

  .done {
    --c: var(--k-lamp-done);
    width: 8px;
    height: 8px;
  }

  .activity {
    --c: var(--k-lamp-activity);
    width: 6px;
    height: 6px;
  }
</style>
