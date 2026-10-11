<script module lang="ts">
  import { msg } from '$lib/i18n/index.svelte';

  // shortcut: the core's toasts carry English text; the static ones below are translated by their exact
  // text, dynamic ones and `KeltaError.message` show as sent (OS notifications and the macOS menu are
  // translated in kelta-proto's i18n.rs). Upgrade by sending message ids from the core.
  export const CORE_TEXTS = [
    msg("Claude Code detected. Let your Claude sessions use Kelta's ticket tools?"),
    msg('Tools moved to your global settings'),
    msg('Ticket sources moved out of your projects'),
    msg('A plugin screen stopped responding and was closed'),
    msg('Claude is working: it gets your message when it stops.'),
    msg('Show the Claude sessions you start outside Kelta in Now?'),
    msg('Install'),
    msg('Never'),
    msg('Review'),
    msg('Enter token'),
    msg('Yes'),
  ];
</script>

<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { Toast } from '$lib/gen';

  import IconButton from './IconButton.svelte';
  import Kbd from './Kbd.svelte';

  interface Props {
    toast: Toast;
    ondismiss: () => void;
    /** Called with the toast action (`command` is an ActionId or plugin command id). */
    onaction?: (action: NonNullable<Toast['action']>) => void;
    /** Called for the toast's `secondary` action. */
    onsecondary?: () => void;
    /** Chord that runs this toast's action (`toast.run_last`), printed on the button. */
    kbd?: string | null;
  }

  let { toast, ondismiss, onaction, onsecondary, kbd = null }: Props = $props();
</script>

<div class="k-toast {toast.level}" role={toast.level === 'error' ? 'alert' : 'status'}>
  <span class="mark" aria-hidden="true"></span>
  <p class="text k-selectable">{t(toast.text)}</p>
  {#if toast.action && onaction}
    <button type="button" class="action" onclick={() => toast.action && onaction?.(toast.action)}>
      {t(toast.action.label)}
      {#if kbd}<Kbd chord={kbd} />{/if}
    </button>
  {/if}
  {#if toast.secondary && onsecondary}
    <button type="button" class="action" onclick={onsecondary}>{t(toast.secondary.label)}</button>
  {/if}
  <IconButton icon="x" label={t('Dismiss')} size="sm" onclick={ondismiss} />
</div>

<style>
  .k-toast {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 400px;
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
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
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
