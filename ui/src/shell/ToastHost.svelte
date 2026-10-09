<script lang="ts">
  import { dispatch } from '$lib/actions';
  import type { Toast as ToastData } from '$lib/gen';
  import { toasts } from '$lib/stores';
  import { Toast } from '$lib/ui';

  async function run(id: number, action: NonNullable<ToastData['action']>): Promise<void> {
    toasts.dismiss(id);
    const args =
      action.args && typeof action.args === 'object' && !Array.isArray(action.args)
        ? (action.args as Record<string, unknown>)
        : undefined;
    try {
      const handled = await dispatch(action.command, args);
      if (!handled) toasts.warn(`No handler for “${action.label}”`);
    } catch (err) {
      toasts.error(err, action.label);
    }
  }
</script>

<div class="toasts" aria-live="polite" data-testid="toasts">
  {#each toasts.list as entry (entry.id)}
    <Toast
      toast={entry.toast}
      ondismiss={() => toasts.dismiss(entry.id)}
      onaction={(a) => void run(entry.id, a)}
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
