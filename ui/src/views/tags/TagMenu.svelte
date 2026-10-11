<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { dispatch } from '$lib/actions';
  import { tags } from '$lib/stores';
  import type { TagKind } from '$lib/stores/tags.svelte';
  import { Menu, type MenuItem } from '$lib/ui';

  // Puts a local tag on an item or takes it off (#349), one per pick; with no tag yet, the way to Settings.
  interface Props {
    kind: TagKind;
    itemKey: string;
    x: number;
    y: number;
    onclose: () => void;
  }

  let { kind, itemKey, x, y, onclose }: Props = $props();

  const items = $derived<MenuItem[]>(
    tags.value.tags.length === 0
      ? [{ id: '', label: t('Create tags in Settings'), icon: 'settings' }]
      : tags.value.tags.map((tag) => ({
          id: tag.name,
          label: tag.name,
          icon: tags.has(kind, itemKey, tag.name) ? 'check' : 'square',
        })),
  );
</script>

<Menu
  {items}
  {x}
  {y}
  label={t('Tags')}
  onselect={(id) =>
    void (id ? tags.toggle(kind, itemKey, id) : dispatch('settings.open', { section: 'tags' }))}
  {onclose}
/>
