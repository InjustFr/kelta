<script lang="ts">
  import { effectiveChords } from '$lib/keys/manager';
  import { settings, toasts } from '$lib/stores';
  import { fadeMove, glide, Toast } from '$lib/ui';

  import { viewKey } from './nav';

  // The key that runs the last toast's action is printed on that toast's button (FLOW §7.1).
  const runLastKey = $derived(effectiveChords('toast.run_last', settings.value()?.keys ?? null)[0] ?? null);
  // Info toasts belong to the view they were raised in: leaving it closes them.
  toasts.where = viewKey;
  $effect(() => {
    const view = viewKey();
    if (view) toasts.leave(view);
  });
  const lastId = $derived(toasts.lastActionable?.id ?? null);
</script>

<div class="toasts" aria-live="polite" data-testid="toasts">
  {#each toasts.list as entry (entry.id)}
    <!-- A toast slides in from the edge (Toast's CSS), fades out, and the stack closes up (#398). -->
    <div animate:glide out:fadeMove={{ x: 16 }}>
      <Toast
        toast={entry.toast}
        ondismiss={() => toasts.dismiss(entry.id)}
        onaction={() => void toasts.run(entry.id)}
        onsecondary={() => void toasts.run(entry.id, true)}
        kbd={entry.id === lastId ? runLastKey : null}
      />
    </div>
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
