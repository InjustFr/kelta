<script lang="ts">
  import type { Snippet } from 'svelte';

  import Icon from './Icon.svelte';

  interface Props {
    title: string;
    icon?: string;
    /** Secondary text (what to do next). */
    body?: string;
    /** Action buttons. */
    actions?: Snippet;
  }

  let { title, icon = 'inbox', body, actions }: Props = $props();
</script>

<div class="k-empty" role="status">
  <p class="title"><Icon name={icon} size={20} />{title}</p>
  {#if body}<p class="body">{body}</p>{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</div>

<style>
  /* One sentence and one action, at the top-left of the pane: no illustration, no centring. */
  .k-empty {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-2);
    max-width: 480px;
    padding: var(--k-space-7) var(--k-space-6);
    color: var(--k-fg-muted);
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

  .title :global(.k-icon) {
    color: var(--k-fg-subtle);
  }

  .body {
    margin: 0;
    max-width: 52ch;
    color: var(--k-fg-muted);
  }

  .actions {
    display: flex;
    gap: var(--k-space-3);
    margin-top: var(--k-space-3);
  }
</style>
