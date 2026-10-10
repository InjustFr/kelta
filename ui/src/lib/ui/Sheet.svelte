<script lang="ts">
  import type { Snippet } from 'svelte';

  import IconButton from './IconButton.svelte';
  import { focusModal, modalKeydown } from './modal';

  interface Props {
    title: string;
    /** Side panel on the right (default) or a centered top sheet (palette-like). */
    side?: 'right' | 'top';
    width?: number;
    onclose: () => void;
    children: Snippet;
    actions?: Snippet;
  }

  let { title, side = 'right', width = 560, onclose, children, actions }: Props = $props();

  const uid = $props.id();
  const titleId = `k-sheet-${uid}`;
  let el = $state<HTMLElement>();

  $effect(() => (el ? focusModal(el) : undefined));

  const onkeydown = (e: KeyboardEvent): void => modalKeydown(e, el, onclose);
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
  {#if side === 'top'}
    <!-- Palette-like: the input is the top edge; Esc and the backdrop close it. -->
    <h2 id={titleId} class="k-visually-hidden">{title}</h2>
  {:else}
    <header>
      <h2 id={titleId}>{title}</h2>
      <IconButton icon="x" label="Close" size="sm" onclick={onclose} />
    </header>
  {/if}
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
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
  }

  /* The container only receives focus as a landing spot; its content carries the focus ring. */
  .k-sheet:focus-visible {
    outline: none;
  }

  .k-sheet.right {
    top: 0;
    right: 0;
    bottom: 0;
    border-radius: var(--k-radius-lg) 0 0 var(--k-radius-lg);
    animation: k-sheet-in var(--k-duration) ease-out;
  }

  .k-sheet.top {
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    max-height: 75vh;
    border-radius: var(--k-radius-lg);
    animation: k-float-in var(--k-duration) ease-out;
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
    font-weight: var(--k-weight-strong);
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
    padding: var(--k-space-3) var(--k-space-5) var(--k-space-5);
  }
</style>
