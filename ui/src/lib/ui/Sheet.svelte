<script lang="ts">
  import type { Snippet } from 'svelte';

  import IconButton from './IconButton.svelte';

  interface Props {
    title: string;
    /** Side panel on the right (default) or a centered top sheet (palette-like). */
    side?: 'right' | 'top';
    width?: number;
    onclose: () => void;
    children: Snippet;
    actions?: Snippet;
  }

  let { title, side = 'right', width = 520, onclose, children, actions }: Props = $props();

  const uid = $props.id();
  const titleId = `k-sheet-${uid}`;
  let el = $state<HTMLElement>();

  $effect(() => {
    const first = el?.querySelector<HTMLElement>('[autofocus], input, textarea, select');
    (first ?? el)?.focus();
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="k-backdrop" onclick={onclose}></div>
<div
  bind:this={el}
  class="k-sheet {side}"
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
    z-index: var(--k-z-sheet);
    background: var(--k-overlay);
  }

  .k-sheet {
    position: fixed;
    z-index: var(--k-z-sheet);
    display: flex;
    flex-direction: column;
    background: var(--k-bg-elev);
    border: 1px solid var(--k-border);
    box-shadow: var(--k-shadow);
  }

  .k-sheet.right {
    top: 0;
    right: 0;
    bottom: 0;
    border-width: 0 0 0 1px;
  }

  .k-sheet.top {
    top: 10vh;
    left: 50%;
    transform: translateX(-50%);
    max-height: 75vh;
    border-radius: var(--k-radius-lg);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--k-space-4) var(--k-space-4) var(--k-space-3) var(--k-space-5);
    border-bottom: 1px solid var(--k-border);
  }

  h2 {
    margin: 0;
    font-size: var(--k-font-size-lg);
    font-weight: 600;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--k-space-4) var(--k-space-5);
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-5);
    border-top: 1px solid var(--k-border);
  }
</style>
