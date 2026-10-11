<script lang="ts">
  // The one settings group header: a 14/600 title (13/600 muted when nested), optional meta and
  // actions on the same line, a one-line description, then rows or cards.
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';

  interface Props extends HTMLAttributes<HTMLElement> {
    heading: string;
    headingId?: string;
    description?: string;
    /** A group inside a group: smaller title, its body indented behind a rule. */
    nested?: boolean;
    /** After the title (a badge). */
    meta?: Snippet;
    /** Right of the title (a button). */
    actions?: Snippet;
    children?: Snippet;
  }

  let { heading, headingId, description, nested = false, meta, actions, children, ...rest }: Props = $props();
</script>

<section class="s-group" class:nested {...rest}>
  <header>
    <svelte:element this={nested ? 'h4' : 'h3'} id={headingId}>{heading}</svelte:element>
    {@render meta?.()}
    {#if actions}<span class="actions">{@render actions()}</span>{/if}
  </header>
  {#if description}<p class="desc">{description}</p>{/if}
  {#if children}<div class="body">{@render children()}</div>{/if}
</section>

<style>
  .s-group {
    margin-top: var(--k-space-6);
  }

  .s-group.nested {
    margin-top: var(--k-space-4);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    min-height: var(--k-control-height-sm);
  }

  h3,
  h4 {
    margin: 0;
    font-weight: var(--k-weight-strong);
  }

  h3 {
    font-size: var(--k-font-size);
  }

  h4 {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .actions {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    margin-left: auto;
  }

  .desc {
    margin: var(--k-space-1) 0 0;
    max-width: var(--k-measure);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .body {
    display: flex;
    flex-direction: column;
    margin-top: var(--k-space-2);
  }

  .nested > .body {
    padding-left: var(--k-space-4);
    border-left: 2px solid var(--k-border);
  }
</style>
