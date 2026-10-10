<script lang="ts">
  import type { Snippet } from 'svelte';

  import IconButton from './IconButton.svelte';
  import { focusModal, modalKeydown } from './modal';

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

  $effect(() => (el ? focusModal(el) : undefined));

  const onkeydown = (e: KeyboardEvent): void => modalKeydown(e, el, onclose);
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
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
  }

  .k-dialog:focus-visible {
    outline: none;
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
    padding: var(--k-space-3) var(--k-space-5) var(--k-space-5);
  }
</style>
