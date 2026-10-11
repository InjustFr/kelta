<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { tags } from '$lib/stores';
  import type { TagKind } from '$lib/stores/tags.svelte';
  import { resolveProjectColor } from '$lib/theme';

  // Toolbar filter by local tag (#349): one chip per tag the list's items carry, with its count.
  interface Props {
    kind: TagKind;
    keys: readonly string[];
    value: string | null;
    onchange: (name: string | null) => void;
  }

  let { kind, keys, value, onchange }: Props = $props();

  // The picked chip stays while it matches nothing, so it can be turned off.
  const chips = $derived(tags.counts(kind, keys).filter((c) => c.n > 0 || c.tag.name === value));
</script>

{#if chips.length > 0}
  <span class="tags" role="group" aria-label={t('Filter by tag')}>
    {#each chips as c (c.tag.name)}
      <button
        type="button"
        class="chip"
        aria-pressed={value === c.tag.name}
        data-tag={c.tag.name}
        title={t('Only items tagged {name}', { name: c.tag.name })}
        onclick={() => onchange(value === c.tag.name ? null : c.tag.name)}
        ><span class="dot" style:background={resolveProjectColor(c.tag.color) ?? 'var(--k-fg-subtle)'}
        ></span>{c.tag.name}<span class="k-num n">{c.n}</span></button
      >
    {/each}
  </span>
{/if}

<style>
  .tags {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--k-space-1);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--k-control-height-sm);
    padding: 0 var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: 999px;
    background: transparent;
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-xs);
    cursor: pointer;
  }

  .chip:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .chip[aria-pressed='true'] {
    border-color: var(--k-accent);
    background: var(--k-bg-selected);
    color: var(--k-fg);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .n {
    color: var(--k-fg-subtle);
  }
</style>
