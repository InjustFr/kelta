<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';

  import type { WorkItem } from '$lib/gen';
  import { relativeTime } from '$lib/ui';

  import { statusLabel } from '../../../shell/labels';
  import { claudeOf } from '../live';
  import Section from './Section.svelte';

  let { item, open }: { item: WorkItem; open: boolean } = $props();

  const claude = $derived(claudeOf(item));
  const summary = $derived(
    claude
      ? [statusLabel(claude.status), item.claude_at ? relativeTime(Date.parse(item.claude_at)) : null]
          .filter(Boolean)
          .join(' · ')
      : t('Not started'),
  );
</script>

<Section title={t('Claude')} {summary} {open} testid="story-claude" />
