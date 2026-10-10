<script lang="ts">
  import type { LampLevel } from '$lib/stores/reducers';

  // Lamp (DESIGN §6.1): each state has its own silhouette so colour is never the only signal.
  interface Props {
    level: LampLevel;
    title?: string;
  }

  let { level, title }: Props = $props();

  const LABELS: Record<LampLevel, string> = {
    none: '',
    activity: 'activity',
    done: 'done',
    working: 'working',
    error: 'error',
    needs_input: 'needs input',
  };
</script>

{#if level !== 'none'}
  <span
    class="lamp {level}"
    role="img"
    aria-label={title ?? LABELS[level]}
    title={title ?? LABELS[level]}
    data-attention={level}
  ></span>
{/if}

<style>
  .lamp {
    display: inline-block;
    flex: none;
    width: 8px;
    height: 8px;
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

  .error {
    --c: var(--k-lamp-error);
    width: 7px;
    height: 7px;
    border-radius: 0;
    transform: rotate(45deg);
  }

  .done {
    --c: var(--k-lamp-done);
    width: 6px;
    height: 6px;
  }

  .activity {
    --c: var(--k-lamp-activity);
    width: 4px;
    height: 4px;
  }
</style>
