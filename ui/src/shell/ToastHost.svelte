<script lang="ts">
  import { effectiveChords } from '$lib/keys/manager';
  import { settings, toasts } from '$lib/stores';
  import { Toast } from '$lib/ui';

  // The key that runs the last toast's action is printed on that toast's button (FLOW §7.1).
  const runLastKey = $derived(effectiveChords('toast.run_last', settings.value()?.keys ?? null)[0] ?? null);
  const lastId = $derived(toasts.lastActionable?.id ?? null);
</script>

<div class="toasts" aria-live="polite" data-testid="toasts">
  {#each toasts.list as entry (entry.id)}
    <Toast
      toast={entry.toast}
      ondismiss={() => toasts.dismiss(entry.id)}
      onaction={() => void toasts.run(entry.id)}
      kbd={entry.id === lastId ? runLastKey : null}
    />
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: var(--k-space-5);
    bottom: calc(var(--k-statusbar-height) + var(--k-space-4));
    z-index: var(--k-z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    pointer-events: none;
  }

  .toasts > :global(*) {
    pointer-events: auto;
  }
</style>
