<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { Layer } from '$lib/gen';
  import Badge from '$lib/ui/Badge.svelte';

  interface Props {
    source: Layer;
    path?: string;
  }

  let { source, path = '' }: Props = $props();

  const LABEL: Record<Layer, string> = $derived({
    default: t('Default'),
    plugin: t('Plugin'),
    global: t('Global'),
    project: t('Project'),
    repo: t('Repo'),
    runtime: t('Runtime'),
  });
  const TONE: Record<Layer, 'neutral' | 'info' | 'accent' | 'ok' | 'warn' | 'danger'> = {
    default: 'neutral',
    plugin: 'info',
    global: 'accent',
    project: 'ok',
    repo: 'warn',
    runtime: 'danger',
  };
  const HELP: Record<Layer, string> = $derived({
    default: t('Compiled default'),
    plugin: t('Default supplied by a plugin'),
    global: t('Set in config.toml'),
    project: t('Set in the project file'),
    repo: t('Set in the repo-local .kelta/config.toml'),
    runtime: t('Set by an environment variable or command-line flag (not saved)'),
  });
</script>

<!-- Quiet layer model: a default value carries no chip, so the keys you set are the ones marked. -->
<span class="source" data-testid="source-badge" data-source={source} data-path={path}>
  {#if source === 'default'}
    <span class="k-visually-hidden">{LABEL[source]}</span>
  {:else}
    <Badge tone={TONE[source]} title={HELP[source]}>{LABEL[source]}</Badge>
  {/if}
</span>

<style>
  .source {
    display: inline-flex;
  }
</style>
