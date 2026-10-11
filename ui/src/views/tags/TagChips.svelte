<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { tags } from '$lib/stores';
  import type { TagKind } from '$lib/stores/tags.svelte';
  import { resolveProjectColor } from '$lib/theme';

  // An item's local tags (#349), as small chips in the tag's colour.
  let { kind, itemKey }: { kind: TagKind; itemKey: string } = $props();
</script>

{#each tags.of(kind, itemKey) as tag (tag.name)}<span
    class="tag"
    data-tag={tag.name}
    title={t('Local tag {name}', { name: tag.name })}
    ><span class="dot" style:background={resolveProjectColor(tag.color) ?? 'var(--k-fg-subtle)'}
    ></span>{tag.name}</span
  >{/each}

<style>
  .tag {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 4px;
    max-width: 120px;
    margin-left: var(--k-space-1);
    padding: 0 6px;
    border: 1px solid var(--k-border);
    border-radius: 999px;
    overflow: hidden;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
    line-height: 16px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
</style>
