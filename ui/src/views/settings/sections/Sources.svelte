<script lang="ts">
  // Settings → Sources: the global `[[sources]]`, one card per board: label, board (account and
  // views), columns, steps, the projects it feeds and the routes of its tickets to a repo.
  import { t } from '$lib/i18n/index.svelte';
  import { tick } from 'svelte';

  import type { SettingsSectionProps } from '$app/registry';
  import type { JsonValue, RepoMatch, Source, SourceRoute } from '$lib/gen';
  import { projects, settings, toasts, ui } from '$lib/stores';
  import { Button, IconButton, Select, TextInput } from '$lib/ui';
  import Icon from '$lib/ui/Icon.svelte';

  import { confirms } from '../../../shell/confirm.svelte';
  import { onSourceRequest, takeSourceRequest } from '../actions';
  import Control from '../fields/Control.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import SettingsCard from '../layout/SettingsCard.svelte';
  import SettingsDisclosure from '../layout/SettingsDisclosure.svelte';
  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SettingsRow from '../layout/SettingsRow.svelte';
  import { descriptionOf, entrySchema, nodeAt, type SchemaNode } from '../lib/schema';
  import { editSource, saveSources } from '../lib/sources.svelte';
  import SectionForm from '../SectionForm.svelte';
  import PipelineSteps from '../PipelineSteps.svelte';
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
  const MATCH: { value: MatchKey; label: string }[] = $derived([
    { value: 'component', label: t('Component') },
    { value: 'label', label: t('Label') },
    { value: 'type', label: t('Type') },
  ]);
  const matchOf = (r: SourceRoute): MatchKey =>
    MATCH.find((m) => r.match[m.value] != null)?.value ?? 'component';

  // Settings → Sources from a toast or a "Fed by" link: that source's card comes into view.
  let wanted = $state(takeSourceRequest());
  // Settings already on Sources: the retarget does not remount this section.
  $effect(() => onSourceRequest(() => (wanted = takeSourceRequest())));
  $effect(() => {
    const req = wanted;
    if (!req || !all.some((s) => s.id === req.id)) return;
    wanted = null; // once: later saves replace `all` and must not scroll back
    void tick().then(() => {
      const card = document.querySelector(`[data-source-card="${CSS.escape(req.id)}"]`);
      (req.steps ? card?.querySelector('[data-testid="pipeline-steps"]') : card)?.scrollIntoView?.({
        block: 'start',
      });
    });
  });

  const setTracker = (s: Source, key: 'columns', v: JsonValue | null) =>
    editSource(s.id, (x) => ({
      ...x,
      tracker: { ...x.tracker, [key]: (v ?? []) as unknown as Source['tracker']['columns'] },
    }));

  /** Unticking a project drops its routes too (a route must name one of the source's projects). */
  const toggleProject = (s: Source, pid: string) =>
    editSource(s.id, (x) =>
      x.projects.includes(pid)
        ? {
            ...x,
            projects: x.projects.filter((p) => p !== pid),
            routes: x.routes.filter((r) => r.project !== pid),
          }
        : { ...x, projects: [...x.projects, pid] },
    );

  const setRoutes = (s: Source, routes: SourceRoute[]) => editSource(s.id, (x) => ({ ...x, routes }));
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
    void setRoutes(s, [...s.routes, { match, project, repo: firstRepo(project), options: [] }]);
  }

  async function remove(s: Source): Promise<void> {
    const answer = await confirms.ask({
      title: t('Remove {name}?', { name: s.label }),
      body: t('Kelta stops listing its tickets. Its views and queries are not kept.'),
      tone: 'danger',
      actions: [{ id: 'remove', label: t('Remove source'), variant: 'danger' }],
    });
    if (answer !== 'remove') return;
    try {
      await saveSources(all.filter((x) => x.id !== s.id));
    } catch (err) {
      toasts.error(err, t('Could not remove the ticket source'));
    }
  }
</script>

<SectionForm sectionId="sources" {...props} skip={['sources']}>
  {#snippet before()}
    {#if globalOnly}
      <div class="notice" role="status" data-testid="sources-global-only">
        <Icon name="info" size={16} />
        <span>{t('Ticket sources are global settings.')}</span>
        <Button size="sm" onclick={() => editor.target({ layer: 'global', repoId: null })}
          >{t('Change in All projects →')}</Button
        >
      </div>
      <ul class="muted" data-testid="sources-global-value">
        {#each all as s (s.id)}<li>{s.label}</li>{:else}<li>{t('No ticket source yet.')}</li>{/each}
      </ul>
    {:else}
      <fieldset class="layer">
        {#if all.length === 0}
          <p class="muted">{t('No ticket source yet.')}</p>
        {/if}
        {#each all as s (s.id)}
          <SettingsCard data-testid="source-card" data-source-card={s.id}>
            {#snippet head()}
              <TextInput
                aria-label={t('Label')}
                value={s.label}
                onchange={(e) => {
                  const label = (e.currentTarget as HTMLInputElement).value.trim();
                  if (label && label !== s.label) void editSource(s.id, (x) => ({ ...x, label }));
                }}
              />
            {/snippet}
            {#snippet actions()}
              <IconButton
                icon="trash"
                label={t('Remove {name}', { name: s.label })}
                size="sm"
                onclick={() => void remove(s)}
              />
            {/snippet}

            <TrackerSources sourceId={s.id} />

            <SettingsRow label={t('Projects')}>
              {#snippet control()}
                <div class="chips" role="group" aria-label={t('Projects {name} feeds', { name: s.label })}>
                  {#each editable as p (p.id)}
                    <Button
                      size="sm"
                      variant={s.projects.includes(p.id) ? 'primary' : 'secondary'}
                      aria-pressed={s.projects.includes(p.id)}
                      onclick={() => void toggleProject(s, p.id)}>{p.name}</Button
                    >
                  {:else}
                    <span class="muted">{t('No project yet.')}</span>
                  {/each}
                </div>
              {/snippet}
              <p>{t('Its tickets show in these projects.')}</p>
            </SettingsRow>

            <SettingsGroup
              heading={t('Routes')}
              description={t(
                'The first route that matches a ticket picks the repository its work starts in.',
              )}
            >
              {#snippet actions()}
                <Button size="sm" icon="plus" disabled={s.projects.length === 0} onclick={() => addRoute(s)}
                  >{t('Add route')}</Button
                >
              {/snippet}
              {#if s.routes.length > 0}
                <table class="routes" data-testid="source-routes">
                  <thead>
                    <tr
                      ><th>{t('Match')}</th><th>{t('Value')}</th><th>{t('Project')}</th><th
                        >{t('Repository')}</th
                      ><th></th></tr
                    >
                  </thead>
                  <tbody>
                    {#each s.routes as r, i (i)}
                      {@const key = matchOf(r)}
                      <tr data-testid="source-route">
                        <td>
                          <Select
                            label={t('Match of route {n}', { n: i + 1 })}
                            value={key}
                            options={MATCH}
                            onchange={(k) => void setMatch(s, i, k, r.match[key] ?? '')}
                          />
                        </td>
                        <td>
                          <TextInput
                            label={t('Value of route {n}', { n: i + 1 })}
                            value={r.match[key] ?? ''}
                            onchange={(e) =>
                              void setMatch(s, i, key, (e.currentTarget as HTMLInputElement).value)}
                          />
                        </td>
                        <td>
                          <Select
                            label={t('Project of route {n}', { n: i + 1 })}
                            value={r.project}
                            options={s.projects.map((p) => ({
                              value: p,
                              label: projects.byId(p)?.name ?? p,
                            }))}
                            onchange={(p) => void patchRoute(s, i, { project: p, repo: firstRepo(p) })}
                          />
                        </td>
                        <td>
                          <Select
                            label={t('Repository of route {n}', { n: i + 1 })}
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
                            label={t('Move route {n} up', { n: i + 1 })}
                            size="sm"
                            disabled={i === 0}
                            onclick={() => moveRoute(s, i, -1)}
                          />
                          <IconButton
                            icon="arrow-down"
                            label={t('Move route {n} down', { n: i + 1 })}
                            size="sm"
                            disabled={i === s.routes.length - 1}
                            onclick={() => moveRoute(s, i, 1)}
                          />
                          <IconButton
                            icon="trash"
                            label={t('Remove route {n}', { n: i + 1 })}
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
            </SettingsGroup>

            <PipelineSteps sourceId={s.id} />

            {#if tracker}
              {@const columns = nodeAt(tracker, ['columns'])}
              <SettingsDisclosure label={t('Columns')}>
                {#if columns}
                  <SettingsRow label={t('Columns')}>
                    {#snippet control()}
                      <Control
                        node={columns}
                        value={s.tracker.columns as unknown as JsonValue}
                        label={t('Columns of {name}', { name: s.label })}
                        onchange={(v) => void setTracker(s, 'columns', v)}
                      />
                    {/snippet}
                    <p>{descriptionOf(columns)}</p>
                  </SettingsRow>
                {/if}
              </SettingsDisclosure>
            {/if}
          </SettingsCard>
        {/each}
        <div class="add">
          <Button variant="primary" icon="plus" onclick={() => ui.openSheet('tracker.source_picker', {})}
            >{t('Add source')}</Button
          >
        </div>
      </fieldset>
    {/if}
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
