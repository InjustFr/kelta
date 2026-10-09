<script lang="ts">
  import type { Snippet } from 'svelte';

  import IconButton from './IconButton.svelte';

  interface Props {
    title: string;
    /** Destructive confirmations use `danger` styling on the title bar. */
    tone?: 'default' | 'danger';
    width?: number;
    onclose: () => void;
    children: Snippet;
    /** Footer buttons. */
    actions?: Snippet;
  }

  let { title, tone = 'default', width = 440, onclose, children, actions }: Props = $props();

  const uid = $props.id();
  const titleId = `k-dialog-${uid}`;
  let el = $state<HTMLDivElement>();
  let previous: Element | null = null;

  $effect(() => {
    previous = document.activeElement;
    const first = el?.querySelector<HTMLElement>(
      '[autofocus], input, textarea, select, button.primary, button',
    );
    (first ?? el)?.focus();
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    } else if (e.key === 'Tab' && el) {
      // Focus trap.
      const focusables = [
        ...el.querySelectorAll<HTMLElement>('button, input, textarea, select, [tabindex="0"]'),
      ].filter((f) => !f.hasAttribute('disabled'));
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (!first || !last) return;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="k-backdrop" onclick={onclose}></div>
<div
  bind:this={el}
  class="k-dialog {tone}"
  role="dialog"
  aria-modal="true"
  aria-labelledby={titleId}
  tabindex="-1"
  style:width="min({width}px, calc(100vw - 32px))"
  {onkeydown}
>
  <header>
    <h2 id={titleId}>{title}</h2>
    <IconButton icon="x" label="Close" size="sm" onclick={onclose} />
  </header>
  <div class="body">{@render children()}</div>
  {#if actions}<footer>{@render actions()}</footer>{/if}
</div>

<style>
  .k-backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--k-z-dialog);
    background: var(--k-overlay);
  }

  .k-dialog {
    position: fixed;
    z-index: var(--k-z-dialog);
    top: 18vh;
    left: 50%;
    transform: translateX(-50%);
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-elev);
    box-shadow: var(--k-shadow);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--k-space-4) var(--k-space-4) var(--k-space-3) var(--k-space-5);
  }

  h2 {
    margin: 0;
    font-size: var(--k-font-size-lg);
    font-weight: 600;
  }

  .danger h2 {
    color: var(--k-danger);
  }

  .body {
    padding: 0 var(--k-space-5) var(--k-space-4);
    overflow: auto;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-5) var(--k-space-4);
    border-top: 1px solid var(--k-border);
  }
</style>
