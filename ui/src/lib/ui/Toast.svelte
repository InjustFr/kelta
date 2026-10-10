<script lang="ts">
  import type { Toast } from '$lib/gen';

  import IconButton from './IconButton.svelte';

  interface Props {
    toast: Toast;
    ondismiss: () => void;
    /** Called with the toast action (`command` is an ActionId or plugin command id). */
    onaction?: (action: NonNullable<Toast['action']>) => void;
  }

  let { toast, ondismiss, onaction }: Props = $props();
</script>

<div class="k-toast {toast.level}" role={toast.level === 'error' ? 'alert' : 'status'}>
  <span class="mark" aria-hidden="true"></span>
  <p class="text k-selectable">{toast.text}</p>
  {#if toast.action && onaction}
    <button type="button" class="action" onclick={() => toast.action && onaction?.(toast.action)}>
      {toast.action.label}
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
    padding: var(--k-space-2) var(--k-space-2) var(--k-space-2) var(--k-space-4);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
  }

  /* Lamp shapes: info = dot, warn and error = diamond. */
  .mark {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--k-info);
  }

  .warn .mark,
  .error .mark {
    border-radius: 0;
    transform: rotate(45deg);
    background: var(--k-warn);
  }

  .error .mark {
    background: var(--k-danger);
  }

  .text {
    flex: 1;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .action {
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: none;
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-accent);
    cursor: pointer;
  }

  .action:hover {
    background: var(--k-bg-hover);
  }
</style>
