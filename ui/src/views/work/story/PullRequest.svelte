<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';

  import type { WorkItem } from '$lib/gen';

  import { prOf } from '../live';
  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  const pr = $derived(prOf(item));
  const num = $derived(pr?.ref.number ?? item.pr_url?.match(/\/(\d+)\/?$/)?.[1] ?? null);
  const state = $derived(
    item.state.kind === 'merged'
      ? t('merged')
      : item.state.kind === 'pr_closed'
        ? t('closed')
        : pr?.draft
          ? t('draft')
          : t('open'),
  );
</script>

<Section
  title={t('Pull request')}
  summary={item.pr_url || pr ? `${num ? `#${num} ` : ''}${state}` : t('No pull request')}
  {open}
  testid="story-pr"
/>
