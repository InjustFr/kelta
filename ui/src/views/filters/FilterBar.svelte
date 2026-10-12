<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';

  import { t } from '$lib/i18n/index.svelte';
  import { Button, Icon } from '$lib/ui';

  import { clearAll, type ActiveFilter } from './filters';

  // The filter bar Now, Reviews and Tickets share (#370): the `/` text filter, whose placeholder names
  // the fields it matches, the view's own filter chips (`children`), then every filter on as a removable
  // chip, with Clear all.
  interface Props extends HTMLInputAttributes {
    value: string;
    input?: HTMLInputElement;
    active: readonly ActiveFilter[];
    children?: Snippet;
  }

  let { value = $bindable(), input = $bindable(), active, children, ...rest }: Props = $props();
</script>

<div class="bar" data-testid="filter-bar">
  <input class="k-filter" bind:this={input} bind:value {...rest} />
  {#if children}<span class="own">{@render children()}</span>{/if}
  {#if active.length > 0}
    <span class="chips" role="group" aria-label={t('Active filters')}>
      {#each active as f (f.id)}
        <span class="chip" data-filter={f.id} title={f.label}
          ><span class="label">{f.label}</span><button
            type="button"
            aria-label={t('Remove filter {name}', { name: f.label })}
            onclick={() => f.clear()}><Icon name="x" size={12} /></button
          ></span
        >
      {/each}
    </span>
    <Button variant="ghost" size="sm" onclick={() => clearAll(active)}>{t('Clear all')}</Button>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    flex: none;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
    padding: var(--k-space-2) var(--k-space-4);
    background: var(--k-bezel-raised);
    font-size: var(--k-font-size-sm);
  }

  .bar .k-filter {
    flex: 0 1 360px;
  }

  /* The view's chips keep to one line and scroll: a dozen statuses must not push the list down. */
  .own {
    display: flex;
    flex: 1 1 0;
    gap: var(--k-space-2);
    min-width: 160px;
    overflow-x: auto;
    scrollbar-width: thin;
  }

  .own :global(.chips) {
    flex: none;
    flex-wrap: nowrap;
  }

  .chips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--k-space-1);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    max-width: 240px;
    height: var(--k-control-height-sm);
    padding: 0 2px 0 var(--k-space-2);
    border: 1px solid var(--k-accent);
    border-radius: 999px;
    background: var(--k-bg-selected);
    color: var(--k-fg);
    font-size: var(--k-font-size-xs);
    white-space: nowrap;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chip button {
    display: inline-flex;
    padding: 2px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--k-fg-muted);
    cursor: pointer;
  }

  .chip button:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }
</style>
