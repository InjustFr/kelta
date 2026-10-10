<script lang="ts">
  import type { Snippet } from 'svelte';

  import type { KeltaError } from '$lib/gen';

  import Button from './Button.svelte';

  interface Props {
    /** A KeltaError, an Error or a message. */
    error: KeltaError | Error | string;
    title?: string;
    onretry?: () => void;
    /** Extra actions ("Open settings", "Open diagnostics"…). */
    actions?: Snippet;
  }

  let { error, title = 'Something went wrong', onretry, actions }: Props = $props();

  const message = $derived(typeof error === 'string' ? error : error.message);
  const code = $derived(typeof error === 'object' && 'code' in error ? String(error.code) : null);
</script>

<div class="k-error" role="alert">
  <p class="title"><span class="mark" aria-hidden="true"></span>{title}</p>
  <p class="message k-selectable">{message}</p>
  {#if code}<p class="code">{code}</p>{/if}
  <div class="actions">
    {#if onretry}<Button icon="refresh-cw" onclick={onretry}>Retry</Button>{/if}
    {@render actions?.()}
  </div>
</div>

<style>
  /* What happened and how to fix it, top-left; the error lamp shape marks it. */
  .k-error {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-2);
    max-width: 480px;
    padding: var(--k-space-7) var(--k-space-6);
  }

  .title {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    margin: 0;
    color: var(--k-fg);
    font-size: var(--k-font-size-lg);
    font-weight: var(--k-weight-strong);
  }

  .mark {
    width: 14px;
    height: 14px;
    margin: 3px;
    transform: rotate(45deg);
    background: var(--k-danger);
  }

  .message {
    margin: 0;
    max-width: 52ch;
    color: var(--k-fg-muted);
    overflow-wrap: anywhere;
  }

  .code {
    margin: 0;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .actions {
    display: flex;
    gap: var(--k-space-3);
    margin-top: var(--k-space-3);
  }
</style>
