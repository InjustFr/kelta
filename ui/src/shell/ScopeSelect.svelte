<script lang="ts">
  // `All projects ▾` in the window's top bar (#409, #429): sets the scope Now, Tickets and Reviews share.
  import { t } from '$lib/i18n/index.svelte';
  import { projects } from '$lib/stores';
  import { Select } from '$lib/ui';

  import { listScope } from './scope.svelte';

  const options = $derived([
    { value: '', label: t('All projects') },
    ...projects.openProjects.filter((p) => !p.builtin).map((p) => ({ value: p.id, label: p.name })),
  ]);
</script>

<span class="scope" data-testid="list-scope">
  <Select
    label={t('Project')}
    value={listScope.project ?? ''}
    {options}
    onchange={(id) => listScope.set(id === '' ? null : id)}
  />
</span>

<style>
  /* A bar control: the label stays for screen readers only, as in `.k-toolbar`. */
  .scope :global(label) {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  .scope :global(select) {
    height: var(--k-control-height-sm);
  }
</style>
