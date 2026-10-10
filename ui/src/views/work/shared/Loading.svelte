<script lang="ts">
  import { Spinner } from '$lib/ui';

  let { label = 'Loading' }: { label?: string } = $props();
</script>

<!-- Appears only after 150 ms (CSS delay, no timer): quick loads never flash a spinner. -->
<div class="loading" role="status" aria-label={label}>
  <Spinner {label} />
  <span>{label}…</span>
</div>

<style>
  .loading {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-5);
    color: var(--k-fg-muted);
    opacity: 0;
    animation: show 0s linear 150ms forwards;
  }

  @keyframes show {
    to {
      opacity: 1;
    }
  }
</style>
