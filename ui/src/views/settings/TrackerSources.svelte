<script lang="ts">
  // Settings → Sources → Board: the account and the views of one source (`[sources.tracker]`).
  // Whose tickets, current iteration and removal save at once to the global `[[sources]]`.
  import type { TrackerView, Who } from '$lib/gen';
  import { settings, ui } from '$lib/stores';
  import { Badge, Button, IconButton, Select, Toggle } from '$lib/ui';

  import { confirms } from '../../shell/confirm.svelte';
  import { editSource, iterationWord, viewAccount } from './lib/sources.svelte';

  interface Props {
    sourceId: string;
  }

  let { sourceId }: Props = $props();

  const all = $derived(settings.value()?.sources ?? []);
  const source = $derived(all.find((s) => s.id === sourceId) ?? null);
  const kindOf = (id: string) => settings.value()?.accounts[id]?.kind;

  const WHO: { value: Who | ''; label: string }[] = [
    { value: 'mine', label: 'Mine' },
    { value: 'unassigned', label: 'Unassigned' },
    { value: 'anyone', label: 'Team' },
  ];
  // A view without `who` keeps its own query (jql, assigned_to, scope): say so rather than guess,
  // and keep the choice for any view with a query so trying a who can be undone.
  const hasQuery = (v: TrackerView) =>
    !!(v.jql || v.query_id != null || v.assigned_to || v.scope || v.search);
  const whoOptions = (v: TrackerView) =>
    v.who && !hasQuery(v) ? WHO : [{ value: '' as const, label: 'Set by query' }, ...WHO];

  const save = (f: (views: TrackerView[]) => TrackerView[]) =>
    editSource(sourceId, (s) => ({ ...s, tracker: { ...s.tracker, views: f(s.tracker.views) } }));

  const patch = (id: string, p: Partial<TrackerView>) =>
    save((views) => views.map((v) => (v.id === id ? { ...v, ...p } : v)));

  /** A view can hold a hand-written query: removal is confirmed (it writes the TOML at once). */
  async function remove(v: TrackerView): Promise<void> {
    const answer = await confirms.ask({
      title: `Remove ${v.label}?`,
      body: `${source?.label ?? 'The source'} stops listing tickets from this view. Its query is not kept.`,
      tone: 'danger',
      actions: [{ id: 'remove', label: 'Remove view', variant: 'danger' }],
    });
    if (answer === 'remove') await save((views) => views.filter((x) => x.id !== v.id));
  }
</script>

{#if source}
  {@const b = source.tracker}
  <section class="tracker" data-testid="tracker-block" data-source-id={source.id}>
    <div class="head">
      <h4>Board</h4>
      <Badge tone="neutral">{b.account}</Badge>
      <Button
        size="sm"
        icon="plus"
        onclick={() => ui.openSheet('tracker.source_picker', { sourceId: source.id })}
        data-testid="add-source">Add view…</Button
      >
    </div>
    {#if b.views.length === 0}
      <p class="empty">No view: {source.label} lists no ticket.</p>
    {:else}
      <ul>
        {#each b.views as v (v.id)}
          {@const acc = viewAccount(b, v)}
          {@const word = iterationWord(kindOf(acc), v)}
          <li data-testid="tracker-source" data-view-id={v.id}>
            <span class="label">{v.label}</span>
            <span class="account">{acc}</span>
            <span class="who">
              <label class="k-visually-hidden" for="who-{source.id}-{v.id}">Whose tickets in {v.label}</label>
              <Select
                value={v.who ?? ''}
                options={whoOptions(v)}
                onchange={(who) => void patch(v.id, { who: who || null })}
                id="who-{source.id}-{v.id}"
              />
            </span>
            {#if word}
              <Toggle
                label="Current {word} only"
                checked={v.current_iteration}
                onchange={(on) => void patch(v.id, { current_iteration: on })}
              />
            {/if}
            <IconButton icon="trash-2" label="Remove {v.label}" size="sm" onclick={() => void remove(v)} />
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .tracker {
    margin-top: var(--k-space-3);
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

  h4 {
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
