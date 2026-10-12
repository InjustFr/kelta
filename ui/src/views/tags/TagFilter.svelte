<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { tags } from '$lib/stores';
  import type { TagKind } from '$lib/stores/tags.svelte';
  import { resolveProjectColor } from '$lib/theme';

  import CountChips from '../filters/CountChips.svelte';

  // Toolbar filter by local tag (#349): one chip per tag the list's items carry, with its count.
  interface Props {
    kind: TagKind;
    keys: readonly string[];
    value: string | null;
    onchange: (name: string | null) => void;
  }

  let { kind, keys, value, onchange }: Props = $props();

  // The picked chip stays while it matches nothing, so it can be turned off.
  const chips = $derived(
    tags
      .counts(kind, keys)
      .filter((c) => c.n > 0 || c.tag.name === value)
      .map((c) => ({
        id: c.tag.name,
        label: c.tag.name,
        n: c.n,
        title: t('Only items tagged {name}', { name: c.tag.name }),
        dot: resolveProjectColor(c.tag.color) ?? 'var(--k-fg-subtle)',
      })),
  );
</script>

<CountChips label={t('Filter by tag')} {chips} {value} {onchange} />
