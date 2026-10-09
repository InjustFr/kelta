<script lang="ts">
  import type { Layer } from '$lib/gen';
  import Badge from '$lib/ui/Badge.svelte';

  interface Props {
    source: Layer;
    path?: string;
  }

  let { source, path = '' }: Props = $props();

  const LABEL: Record<Layer, string> = {
    default: 'Default',
    plugin: 'Plugin',
    global: 'Global',
    project: 'Project',
    repo: 'Repo',
    runtime: 'Runtime',
  };
  const TONE: Record<Layer, 'neutral' | 'info' | 'accent' | 'ok' | 'warn' | 'danger'> = {
    default: 'neutral',
    plugin: 'info',
    global: 'accent',
    project: 'ok',
    repo: 'warn',
    runtime: 'danger',
  };
  const HELP: Record<Layer, string> = {
    default: 'Compiled default',
    plugin: 'Default supplied by a plugin',
    global: 'Set in config.toml',
    project: 'Set in the project file',
    repo: 'Set in the repo-local .kelta/config.toml',
    runtime: 'Set by an environment variable or command-line flag (not saved)',
  };
</script>

<span class="source" data-testid="source-badge" data-source={source} data-path={path}>
  <Badge tone={TONE[source]} title={HELP[source]}>{LABEL[source]}</Badge>
</span>

<style>
  .source {
    display: inline-flex;
  }
</style>
