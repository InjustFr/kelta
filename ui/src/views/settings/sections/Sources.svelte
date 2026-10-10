<script lang="ts">
  // Settings → Sources: the global `[[sources]]`, one card per board: label, board (account and
  // views), columns, status map, the projects it feeds and the routes of its tickets to a repo.
  import { tick } from 'svelte';

  import type { SettingsSectionProps } from '$app/registry';
  import type { JsonValue, RepoMatch, Source, SourceRoute } from '$lib/gen';
  import { projects, settings, toasts, ui } from '$lib/stores';
  import { Button, IconButton, Select, TextInput } from '$lib/ui';
  import Icon from '$lib/ui/Icon.svelte';

  import { confirms } from '../../../shell/confirm.svelte';
  import { takeSourceRequest } from '../actions';
  import Control from '../fields/Control.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import { entrySchema, nodeAt, type SchemaNode } from '../lib/schema';
  import { saveSources, withEdited } from '../lib/sources.svelte';
  import SectionForm from '../SectionForm.svelte';
  import TrackerSources from '../TrackerSources.svelte';

  let props: SettingsSectionProps = $props();

  const editor = useEditor(() => ({ layer: props.layer, projectId: props.projectId, repoId: props.repoId }));
  const globalOnly = $derived(editor.layer !== 'global');
  const all = $derived(settings.value()?.sources ?? []);
  const editable = $derived(projects.list.filter((p) => !p.builtin));
  const tracker = $derived.by((): SchemaNode | null => {
    const list = editor.schema ? nodeAt(editor.schema, ['sources']) : null;
    const item = list ? entrySchema(list) : null;
    return item ? nodeAt(item, ['tracker']) : null;
  });

  type MatchKey = keyof RepoMatch;
  const MATCH: { value: MatchKey; label: string }[] = [
    { value: 'component', label: 'Component' },
    { value: 'label', label: 'Label' },
    { value: 'type', label: 'Type' },
  ];
  const matchOf = (r: SourceRoute): MatchKey =>
    MATCH.find((m) => r.match[m.value] != null)?.value ?? 'component';

  // Settings → Sources from a toast or a "Fed by" link: that source's card comes into view.
  const wanted = takeSourceRequest();
  $effect(() => {
    if (!wanted || !all.some((s) => s.id === wanted)) return;
    void tick().then(() =>
      document
        .querySelector(`[data-source-card="${CSS.escape(wanted)}"]`)
        ?.scrollIntoView?.({ block: 'start' }),
    );
  });

  async function edit(id: string, f: (s: Source) => Source): Promise<void> {
    try {
      await saveSources(withEdited(all, id, f));
    } catch (err) {
      toasts.error(err, 'Could not save the ticket source');
    }
  }

  const setTracker = (s: Source, key: 'columns' | 'status_map', v: JsonValue | null) =>
    edit(s.id, (x) => ({ ...x, tracker: { ...x.tracker, [key]: v ?? (key === 'columns' ? [] : {}) } }));

  /** Unticking a project drops its routes too (a route must name one of the source's projects). */
  const toggleProject = (s: Source, pid: string) =>
    edit(s.id, (x) =>
      x.projects.includes(pid)
        ? {
            ...x,
            projects: x.projects.filter((p) => p !== pid),
            routes: x.routes.filter((r) => r.project !== pid),
          }
        : { ...x, projects: [...x.projects, pid] },
    );

  const setRoutes = (s: Source, routes: SourceRoute[]) => edit(s.id, (x) => ({ ...x, routes }));
  const patchRoute = (s: Source, i: number, p: Partial<SourceRoute>) =>
    setRoutes(
      s,
      s.routes.map((r, j) => (j === i ? { ...r, ...p } : r)),
    );
  const setMatch = (s: Source, i: number, key: MatchKey, value: string) =>
    patchRoute(s, i, { match: { component: null, label: null, type: null, [key]: value } });
  const firstRepo = (pid: string) => projects.byId(pid)?.repos[0]?.id ?? '';

  function moveRoute(s: Source, i: number, delta: number): void {
    const routes = [...s.routes];
    const [r] = routes.splice(i, 1);
    if (r) routes.splice(i + delta, 0, r);
    void setRoutes(s, routes);
  }

  function addRoute(s: Source): void {
    const project = s.projects[0] ?? '';
    const match = { component: '', label: null, type: null };
    void setRoutes(s, [...s.routes, { match, project, repo: firstRepo(project) }]);
  }

  async function remove(s: Source): Promise<void> {
    const answer = await confirms.ask({
      title: `Remove ${s.label}?`,
      body: 'Kelta stops listing its tickets. Its views and queries are not kept.',
      tone: 'danger',
      actions: [{ id: 'remove', label: 'Remove source', variant: 'danger' }],
    });
    if (answer !== 'remove') return;
    try {
      await saveSources(all.filter((x) => x.id !== s.id));
    } catch (err) {
      toasts.error(err, 'Could not remove the ticket source');
    }
  }
</script>

<SectionForm sectionId="sources" {...props} skip={['sources']}>
  {#snippet after()}
    {#if globalOnly}
      <div class="notice" role="status" data-testid="sources-global-only">
        <Icon name="info" size={16} />
        <span>Ticket sources are global settings.</span>
        <Button size="sm" onclick={() => editor.target({ layer: 'global', repoId: null })}
          >Switch to Global</Button
        >
      </div>
    {/if}
    <fieldset class="layer" disabled={globalOnly}>
      {#if all.length === 0}
        <p class="muted">No ticket source yet.</p>
      {/if}
      {#each all as s (s.id)}
        <section class="card" data-testid="source-card" data-source-card={s.id}>
          <div class="head">
            <TextInput
              label="Label"
              value={s.label}
              onchange={(e) => {
                const label = (e.currentTarget as HTMLInputElement).value.trim();
                if (label && label !== s.label) void edit(s.id, (x) => ({ ...x, label }));
              }}
            />
            <IconButton icon="trash-2" label="Remove {s.label}" size="sm" onclick={() => void remove(s)} />
          </div>

          <TrackerSources sourceId={s.id} />

          {#if tracker}
            {@const columns = nodeAt(tracker, ['columns'])}
            {@const statusMap = nodeAt(tracker, ['status_map'])}
            {#if columns}
              <h4>Columns</h4>
              <Control
                node={columns}
                value={s.tracker.columns as unknown as JsonValue}
                label="Columns of {s.label}"
                onchange={(v) => void setTracker(s, 'columns', v)}
              />
            {/if}
            {#if statusMap}
              <h4>Status map</h4>
              <Control
                node={statusMap}
                value={s.tracker.status_map as unknown as JsonValue}
                label="Status map of {s.label}"
                onchange={(v) => void setTracker(s, 'status_map', v)}
              />
            {/if}
          {/if}

          <h4>Projects</h4>
          <div class="chips" role="group" aria-label="Projects {s.label} feeds">
            {#each editable as p (p.id)}
              <Button
                size="sm"
                variant={s.projects.includes(p.id) ? 'primary' : 'secondary'}
                aria-pressed={s.projects.includes(p.id)}
                onclick={() => void toggleProject(s, p.id)}>{p.name}</Button
              >
            {:else}
              <span class="muted">No project yet.</span>
            {/each}
          </div>

          <h4>Routes</h4>
          <p class="muted">The first route that matches a ticket picks the repository its work starts in.</p>
          {#if s.routes.length > 0}
            <table class="routes" data-testid="source-routes">
              <thead>
                <tr><th>Match</th><th>Value</th><th>Project</th><th>Repository</th><th></th></tr>
              </thead>
              <tbody>
                {#each s.routes as r, i (i)}
                  {@const key = matchOf(r)}
                  <tr data-testid="source-route">
                    <td>
                      <Select
                        label="Match of route {i + 1}"
                        value={key}
                        options={MATCH}
                        onchange={(k) => void setMatch(s, i, k, r.match[key] ?? '')}
                      />
                    </td>
                    <td>
                      <TextInput
                        label="Value of route {i + 1}"
                        value={r.match[key] ?? ''}
                        onchange={(e) =>
                          void setMatch(s, i, key, (e.currentTarget as HTMLInputElement).value)}
                      />
                    </td>
                    <td>
                      <Select
                        label="Project of route {i + 1}"
                        value={r.project}
                        options={s.projects.map((p) => ({ value: p, label: projects.byId(p)?.name ?? p }))}
                        onchange={(p) => void patchRoute(s, i, { project: p, repo: firstRepo(p) })}
                      />
                    </td>
                    <td>
                      <Select
                        label="Repository of route {i + 1}"
                        value={r.repo}
                        options={(projects.byId(r.project)?.repos ?? []).map((x) => ({
                          value: x.id,
                          label: x.id,
                        }))}
                        onchange={(repo) => void patchRoute(s, i, { repo })}
                      />
                    </td>
                    <td class="actions">
                      <IconButton
                        icon="arrow-up"
                        label="Move route {i + 1} up"
                        size="sm"
                        disabled={i === 0}
                        onclick={() => moveRoute(s, i, -1)}
                      />
                      <IconButton
                        icon="arrow-down"
                        label="Move route {i + 1} down"
                        size="sm"
                        disabled={i === s.routes.length - 1}
                        onclick={() => moveRoute(s, i, 1)}
                      />
                      <IconButton
                        icon="trash-2"
                        label="Remove route {i + 1}"
                        size="sm"
                        onclick={() =>
                          void setRoutes(
                            s,
                            s.routes.filter((_, j) => j !== i),
                          )}
                      />
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
          <div>
            <Button size="sm" icon="plus" disabled={s.projects.length === 0} onclick={() => addRoute(s)}
              >Add route</Button
            >
          </div>
        </section>
      {/each}
      <div class="add">
        <Button variant="primary" icon="plus" onclick={() => ui.openSheet('tracker.source_picker', {})}
          >Add source</Button
        >
      </div>
    </fieldset>
  {/snippet}
</SectionForm>

<style>
  .layer {
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .notice {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin-top: var(--k-space-4);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .head {
    display: flex;
    align-items: flex-end;
    gap: var(--k-space-3);
  }

  h4 {
    margin: var(--k-space-3) 0 0;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2);
  }

  .muted {
    margin: 0;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .routes {
    border-collapse: collapse;
    width: 100%;
  }

  .routes th {
    text-align: left;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    font-weight: normal;
  }

  .routes td {
    padding: var(--k-space-1) var(--k-space-2) var(--k-space-1) 0;
  }

  .actions {
    white-space: nowrap;
  }

  .add {
    margin-top: var(--k-space-4);
  }
</style>
