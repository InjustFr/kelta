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
  <Icon name={icon} size={28} />
  <p class="title">{title}</p>
  {#if body}<p class="body">{body}</p>{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</div>

<style>
  .k-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--k-space-3);
    height: 100%;
    min-height: 120px;
    padding: var(--k-space-6);
    text-align: center;
    color: var(--k-fg-subtle);
  }

  .title {
    margin: 0;
    color: var(--k-fg);
    font-weight: 600;
  }

  .body {
    margin: 0;
    max-width: 42ch;
    color: var(--k-fg-muted);
  }

  .actions {
    display: flex;
    gap: var(--k-space-3);
    margin-top: var(--k-space-2);
  }
</style>
