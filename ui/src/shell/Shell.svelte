<script lang="ts">
  // SCAFFOLD STUB (L2): the shell root (rail, tab bar, layout renderer, palette, switcher,
  // new-session sheet, status bar, toasts host). Replace entirely; keep the default export.
  import { projects, sessions, toasts } from '$lib/stores';
  import NotImplemented from '$lib/ui/NotImplemented.svelte';
  import Toast from '$lib/ui/Toast.svelte';
</script>

<main class="shell" data-testid="shell">
  <NotImplemented name="Shell" owner="L2" />
  <p class="summary" data-testid="scaffold-summary">
    {projects.list.length} projects · {sessions.all.length} sessions
  </p>
  <div class="toasts">
    {#each toasts.list as entry (entry.id)}
      <Toast toast={entry.toast} ondismiss={() => toasts.dismiss(entry.id)} />
    {/each}
  </div>
</main>

<style>
  .shell {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
    padding: var(--k-space-6);
  }

  .summary {
    margin: 0;
    color: var(--k-fg-muted);
  }

  .toasts {
    position: fixed;
    right: var(--k-space-5);
    bottom: var(--k-space-5);
    z-index: var(--k-z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }
</style>
