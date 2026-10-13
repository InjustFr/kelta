<script lang="ts">
  // Settings › Health (#411): every problem Kelta knows of right now, each with its fix.
  import { t } from '$lib/i18n/index.svelte';
  import { dispatch } from '$lib/actions';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { relativeTime } from '$lib/ui/format';
  import { onMount } from 'svelte';

  import SettingsRow from '../layout/SettingsRow.svelte';
  import { health, type Problem } from '../lib/health.svelte';

  const problems = $derived(health.problems);
  let busy = $state<string | null>(null);

  onMount(() => void health.refresh());

  async function fix(p: Problem): Promise<void> {
    busy = p.id;
    try {
      await dispatch(p.fix.action, p.fix.args);
      await health.refresh();
    } catch (err) {
      toasts.error(err, t('Fixing failed'));
    } finally {
      busy = null;
    }
  }
</script>

<div data-testid="health">
  {#if problems.length === 0}
    <EmptyState title={t('Everything works.')} icon="circle-check" />
  {:else}
    {#each problems as p (p.id)}
      <SettingsRow label={p.what} actions data-testid="health-problem" data-problem={p.id}>
        {#snippet control()}
          <Button size="sm" loading={busy === p.id} onclick={() => fix(p)}>{p.fix.label}</Button>
        {/snippet}
        <p class="where">{p.where}{p.since !== null ? ` · ${relativeTime(p.since)}` : ''}</p>
      </SettingsRow>
    {/each}
  {/if}
</div>

<style>
  .where {
    margin: 0;
    color: var(--k-fg-muted);
  }
</style>
