<script lang="ts">
  // Settings → Projects → Tracker: the ticket sources of one project (`[project.tracker].views`).
  // Whose tickets, current iteration and removal save at once through `project_update`.
  import type { TrackerBinding, TrackerView, Who } from '$lib/gen';
  import { projects, settings, toasts, ui } from '$lib/stores';
  import { Button, IconButton, Select, Toggle } from '$lib/ui';

  import { confirms } from '../../shell/confirm.svelte';
  import { saveTracker, iterationWord, viewAccount } from './lib/sources.svelte';

  interface Props {
    /** Project shown first (the settings pane's project). */
    projectId: string | null;
  }

  let { projectId }: Props = $props();

  const editable = $derived(projects.list.filter((p) => !p.builtin));
  // the block's own pick, dropped once the pane's project changes
  let chosen = $state<{ id: string; from: string | null } | null>(null);
  const shownId = $derived(chosen?.from === projectId ? chosen.id : projectId);
  const project = $derived(editable.find((p) => p.id === shownId) ?? editable[0] ?? null);
  const binding = $derived(project?.tracker ?? null);
  const kindOf = (id: string) => settings.value()?.accounts[id]?.kind;

  const WHO: { value: Who | ''; label: string }[] = [
    { value: 'mine', label: 'Mine' },
    { value: 'unassigned', label: 'Unassigned' },
    { value: 'anyone', label: 'Anyone' },
  ];
  // A view without `who` keeps its own query (jql, assigned_to, scope): say so rather than guess,
  // and keep the choice for any view with a query so trying a who can be undone.
  const hasQuery = (v: TrackerView) =>
    !!(v.jql || v.query_id != null || v.assigned_to || v.scope || v.search);
  const whoOptions = (v: TrackerView) =>
    v.who && !hasQuery(v) ? WHO : [{ value: '' as const, label: 'Set by query' }, ...WHO];

  $effect(() => {
    if (settings.value() === null) void settings.load().catch(() => undefined);
  });

  async function save(b: TrackerBinding, views: TrackerView[]): Promise<void> {
    if (!project) return;
    try {
      await saveTracker(project.id, { ...b, views });
    } catch (err) {
      toasts.error(err, 'Could not save the ticket sources');
    }
  }

  const patch = (b: TrackerBinding, id: string, p: Partial<TrackerView>) =>
    save(
      b,
      b.views.map((v) => (v.id === id ? { ...v, ...p } : v)),
    );

  /** A source can hold a hand-written query: removal is confirmed (it writes the TOML at once). */
  async function remove(b: TrackerBinding, v: TrackerView): Promise<void> {
    const answer = await confirms.ask({
      title: `Remove ${v.label}?`,
      body: `${project?.name ?? 'The project'} stops listing tickets from this source. Its query is not kept.`,
      tone: 'danger',
      actions: [{ id: 'remove', label: 'Remove source', variant: 'danger' }],
    });
    if (answer === 'remove')
      await save(
        b,
        b.views.filter((x) => x.id !== v.id),
      );
  }
</script>

{#if project}
  <section class="tracker" data-testid="tracker-block" data-project-id={project.id}>
    <div class="head">
      <h3>Tracker</h3>
      {#if editable.length > 1}
        <label class="k-visually-hidden" for="tracker-project">Project</label>
        <Select
          value={project.id}
          options={editable.map((p) => ({ value: p.id, label: p.name }))}
          onchange={(id) => (chosen = { id, from: projectId })}
          id="tracker-project"
        />
      {/if}
      <Button
        size="sm"
        icon="plus"
        onclick={() => ui.openSheet('tracker.source_picker', { projectId: project.id })}
        data-testid="add-source">Add source</Button
      >
    </div>
    {#if !binding || binding.views.length === 0}
      <p class="empty">No ticket source for {project.name}.</p>
    {:else}
      {@const b = binding}
      <ul>
        {#each b.views as v (v.id)}
          {@const acc = viewAccount(b, v)}
          {@const word = iterationWord(kindOf(acc), v)}
          <li data-testid="tracker-source" data-view-id={v.id}>
            <span class="label">{v.label}</span>
            <span class="account">{acc}</span>
            <span class="who">
              <label class="k-visually-hidden" for="who-{v.id}">Whose tickets in {v.label}</label>
              <Select
                value={v.who ?? ''}
                options={whoOptions(v)}
                onchange={(who) => void patch(b, v.id, { who: who || null })}
                id="who-{v.id}"
              />
            </span>
            {#if word}
              <Toggle
                label="Current {word} only"
                checked={v.current_iteration}
                onchange={(on) => void patch(b, v.id, { current_iteration: on })}
              />
            {/if}
            <IconButton icon="trash-2" label="Remove {v.label}" size="sm" onclick={() => void remove(b, v)} />
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .tracker {
    margin-top: var(--k-space-5);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    margin-bottom: var(--k-space-2);
  }

  .head :global(.k-button) {
    margin-left: auto;
  }

  h3 {
    margin: 0;
  }

  .empty {
    margin: 0;
    color: var(--k-fg-muted);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--k-space-4);
    min-height: var(--k-row-height);
    border-bottom: 1px solid var(--k-border);
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }
</style>
