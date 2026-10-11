<script lang="ts">
  // The one list-editor card (sources, accounts, projects, tools, triggers, plugins, keyed lists and
  // maps): a head line with the entry's name, its meta and its actions, then an optional body of rows.
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';

  interface Props extends HTMLAttributes<HTMLElement> {
    heading?: string;
    /** The name is an id you could paste in a config file. */
    mono?: boolean;
    /** A disabled entry. */
    dim?: boolean;
    /** After the name: badges, muted meta, or the whole head when there is no heading. */
    head?: Snippet;
    /** Right-aligned buttons. */
    actions?: Snippet;
    children?: Snippet;
  }

  let { heading, mono = false, dim = false, head, actions, children, ...rest }: Props = $props();
</script>

<article class="s-card" class:dim {...rest}>
  <header>
    {#if heading}<strong class="name" class:mono>{heading}</strong>{/if}
    {@render head?.()}
    {#if actions}<span class="actions">{@render actions()}</span>{/if}
  </header>
  {#if children}<div class="body">{@render children()}</div>{/if}
</article>

<style>
  .s-card {
    margin-top: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2) var(--k-space-4);
    min-height: var(--k-control-height);
    padding: var(--k-space-3) var(--k-space-4);
  }

  .name {
    font-weight: var(--k-weight-strong);
  }

  .mono {
    font-family: var(--k-font-mono);
  }

  .actions {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    margin-left: auto;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: var(--k-space-2) var(--k-space-4);
    border-top: 1px solid var(--k-border);
  }

  /* A body whose blocks rendered nothing (an entry not being edited) takes no space. */
  .body:empty {
    display: none;
  }

  .body > :global(.s-group) {
    margin-top: var(--k-space-3);
  }

  .body > :global(.s-row:last-child) {
    border-bottom: none;
  }

  .dim {
    opacity: 0.65;
  }
</style>
