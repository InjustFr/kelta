<script lang="ts">
  import type { Toast } from '$lib/gen';

  import Icon from './Icon.svelte';
  import IconButton from './IconButton.svelte';
  import Kbd from './Kbd.svelte';

  interface Props {
    toast: Toast;
    ondismiss: () => void;
    /** Called with the toast action (`command` is an ActionId or plugin command id). */
    onaction?: (action: NonNullable<Toast['action']>) => void;
    /** Chord that runs this toast's action (`toast.run_last`), printed on the button. */
    kbd?: string | null;
  }

  let { toast, ondismiss, onaction, kbd = null }: Props = $props();

  const icon = $derived(
    toast.level === 'error' ? 'circle-alert' : toast.level === 'warn' ? 'triangle-alert' : 'info',
  );
</script>

<div class="k-toast {toast.level}" role={toast.level === 'error' ? 'alert' : 'status'}>
  <Icon name={icon} size={16} />
  <p class="text k-selectable">{toast.text}</p>
  {#if toast.action && onaction}
    <button type="button" class="action" onclick={() => toast.action && onaction?.(toast.action)}>
      {toast.action.label}
      {#if kbd}<Kbd chord={kbd} />{/if}
    </button>
  {/if}
  <IconButton icon="x" label="Dismiss" size="sm" onclick={ondismiss} />
</div>

<style>
  .k-toast {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 360px;
    max-width: calc(100vw - 32px);
    padding: var(--k-space-3) var(--k-space-3) var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-left: 3px solid var(--k-info);
    border-radius: var(--k-radius);
    background: var(--k-bg-elev);
    box-shadow: var(--k-shadow);
  }

  .warn {
    border-left-color: var(--k-warn);
  }

  .error {
    border-left-color: var(--k-danger);
  }

  .info :global(.k-icon) {
    color: var(--k-info);
  }

  .warn :global(.k-icon:first-child) {
    color: var(--k-warn);
  }

  .error :global(.k-icon:first-child) {
    color: var(--k-danger);
  }

  .text {
    flex: 1;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .action {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    border: none;
    background: transparent;
    color: var(--k-accent);
    font-weight: 600;
    cursor: pointer;
  }
</style>
