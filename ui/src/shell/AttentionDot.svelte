<script lang="ts">
  import type { Attention } from '$lib/gen';

  interface Props {
    level: Attention;
    size?: number;
    title?: string;
  }

  let { level, size = 8, title }: Props = $props();

  const LABELS: Record<Attention, string> = {
    none: '',
    activity: 'activity',
    done: 'done',
    error: 'error',
    needs_input: 'needs input',
  };
</script>

{#if level !== 'none'}
  <span
    class="dot {level}"
    style:width="{size}px"
    style:height="{size}px"
    role="img"
    aria-label={title ?? LABELS[level]}
    title={title ?? LABELS[level]}
    data-attention={level}
  ></span>
{/if}

<style>
  .dot {
    display: inline-block;
    flex: none;
    border-radius: 50%;
  }

  .needs_input {
    background: var(--k-att-needs-input);
  }

  .error {
    background: var(--k-att-error);
  }

  .done {
    background: var(--k-att-done);
  }

  .activity {
    background: var(--k-att-activity);
  }
</style>
