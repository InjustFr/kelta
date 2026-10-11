<script lang="ts">
  // Settings pane (Mod+,): layer selector, section list, search across every owned key, the
  // schema-generated forms and a raw "Edit TOML" mode per layer.
  import { t } from '$lib/i18n/index.svelte';
  import {
    settingsSections,
    type PaneProps,
    type SettingsSection,
    type SettingsSectionProps,
  } from '$app/registry';
  import type { ValidationIssue } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, settings as settingsStore } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { currentPlatform } from '$lib/ui/format';
  import { untrack, type Component } from 'svelte';

  import { takeTomlRequest } from './actions';
  import EditToml from './EditToml.svelte';
  import Field from './fields/Field.svelte';
  import { provideEditor, SettingsEditor, type EditLayer } from './lib/editor.svelte';
  import { COPY, leafFields, descriptionOf, titleOf, enumOptions, type LeafField } from './lib/schema';
  import { SECTION_GROUPS, SECTION_ROOTS, sectionLabel } from './lib/sections';
  import RepoTrust from './RepoTrust.svelte';

  let { projectId, content }: PaneProps<'settings'> = $props();

  const tomlProject = takeTomlRequest();
  const initialTarget = () => ({
    layer: tomlProject ? ('project' as const) : ('global' as const),
    projectId: tomlProject ?? (projectId !== 'home' ? projectId : null),
  });
  const editor = provideEditor(new SettingsEditor(initialTarget()));
  editor.raw = tomlProject !== null;
  const platform = currentPlatform();
  const sections = $derived(settingsSections.filter((s) => !s.platform || s.platform === platform));
  const navGroups = $derived(
    SECTION_GROUPS.map((g) => ({
      label: t(g.label),
      sections: g.ids.flatMap((id) => sections.filter((s) => s.id === id)),
    })).filter((g) => g.sections.length > 0),
  );

  let sectionId = $state<string>('general');
  let query = $state('');
  let Active = $state<Component<SettingsSectionProps> | null>(null);
  let sectionLoading = $state(false);
  let layerIssues = $state<ValidationIssue[]>([]);
  const layerErrors = $derived(layerIssues.filter((i) => i.severity === 'error'));
  const layerWarnings = $derived(layerIssues.filter((i) => i.severity === 'warning'));

  const configurable = $derived(projects.list.filter((p) => !p.builtin));
  const projectOptions = $derived(configurable.map((p) => ({ value: p.id, label: p.name })));
  const currentProject = $derived(projects.byId(editor.projectId ?? ''));
  const repoOptions = $derived((currentProject?.repos ?? []).map((r) => ({ value: r.id, label: r.id })));

  $effect(() => {
    // section from the pane content (e.g. "Open Accounts settings"), also when settings.open
    // retargets this open pane, with its TOML request
    const wanted = content.section;
    if (wanted && sections.some((s) => s.id === wanted)) sectionId = wanted;
    const toml = takeTomlRequest();
    if (toml) {
      editor.raw = true;
      untrack(() => void editor.target({ layer: 'project', projectId: toml, repoId: null }));
    }
  });

  $effect(() => {
    // initial load only: layer / project / repo changes reload through editor.target()
    untrack(() => void editor.load());
  });

  const section = $derived<SettingsSection | undefined>(sections.find((s) => s.id === sectionId));

  $effect(() => {
    const s = section;
    if (!s) return;
    sectionLoading = true;
    void s.load().then((m) => {
      if (section?.id === s.id) {
        Active = m.default as unknown as Component<SettingsSectionProps>;
        sectionLoading = false;
      }
    });
  });

  // Parse / validation problems of the file being edited (last good config stays active).
  $effect(() => {
    const text = editor.doc?.text;
    if (!text) {
      layerIssues = [];
      return;
    }
    void ipc
      .settingsValidate({ layer: editor.layer, text })
      .then((i) => (layerIssues = i))
      .catch(() => (layerIssues = []));
  });

  async function chooseLayer(layer: EditLayer): Promise<void> {
    if (layer === 'global') {
      await editor.target({ layer, repoId: null });
      return;
    }
    const pid = editor.projectId ?? configurable[0]?.id ?? null;
    if (layer === 'repo') {
      const repos = projects.byId(pid ?? '')?.repos ?? [];
      const repo = repos.find((r) => r.primary) ?? repos[0];
      await editor.target({ layer, projectId: pid, repoId: repo?.id ?? null });
    } else {
      await editor.target({ layer, projectId: pid, repoId: null });
    }
  }

  async function chooseProject(id: string): Promise<void> {
    const repos = projects.byId(id)?.repos ?? [];
    const repo = repos.find((r) => r.primary) ?? repos[0];
    await editor.target({ projectId: id, repoId: editor.layer === 'repo' ? (repo?.id ?? null) : null });
  }

  // ---- search ---------------------------------------------------------------------------------
  const allRoots = Object.values(SECTION_ROOTS).flatMap((r) => r ?? []);
  const sectionOfRoot = (root: string): string =>
    t(sections.find((s) => SECTION_ROOTS[s.id]?.includes(root))?.label ?? '');

  function matches(f: LeafField, q: string): boolean {
    const hay = [
      f.path,
      titleOf(f.segs[f.segs.length - 1] ?? ''),
      t(COPY[f.path]?.label ?? ''),
      t(COPY[f.path]?.help ?? ''),
      descriptionOf(f.node),
      ...enumOptions(f.node).map((o) => o.value),
    ]
      .join(' ')
      .toLowerCase();
    return q
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean)
      .every((t) => hay.includes(t));
  }

  const results = $derived.by((): LeafField[] => {
    const q = query.trim();
    if (!q || !editor.schema) return [];
    return leafFields(editor.schema, allRoots)
      .filter((f) => matches(f, q))
      .slice(0, 60);
  });
</script>

<div class="pane" data-testid="settings-pane" data-pane-kind="settings">
  <header class="k-toolbar bar">
    <div class="layers" role="group" aria-label={t('Layer being edited')}>
      <button
        type="button"
        title={t('Applies everywhere')}
        class:active={editor.layer === 'global'}
        aria-pressed={editor.layer === 'global'}
        onclick={() => chooseLayer('global')}
      >
        {t('Global')}
      </button>
      <button
        type="button"
        class:active={editor.layer === 'project'}
        aria-pressed={editor.layer === 'project'}
        disabled={configurable.length === 0}
        title={t('Overrides Global for one project')}
        onclick={() => chooseLayer('project')}
      >
        {t('Project')}
      </button>
      <button
        type="button"
        class:active={editor.layer === 'repo'}
        aria-pressed={editor.layer === 'repo'}
        disabled={configurable.length === 0}
        title={t('Overrides the project for one repository')}
        onclick={() => chooseLayer('repo')}
      >
        {t('Repo')}
      </button>
    </div>
    {#if editor.layer !== 'global'}
      <Select
        value={editor.projectId ?? ''}
        options={projectOptions}
        id="settings-project"
        onchange={(id) => chooseProject(id)}
      />
    {/if}
    {#if editor.layer === 'repo' && repoOptions.length > 0}
      <Select
        value={editor.repoId ?? ''}
        options={repoOptions}
        id="settings-repo"
        onchange={(id) => editor.target({ repoId: id })}
      />
    {/if}
    <label class="search">
      <Icon name="search" size={14} />
      <input
        type="search"
        placeholder={t('Search settings')}
        aria-label={t('Search settings')}
        bind:value={query}
      />
    </label>
    <Button
      size="sm"
      icon="file-code"
      variant={editor.raw ? 'primary' : 'ghost'}
      onclick={() => (editor.raw = !editor.raw)}
      data-testid="edit-toml-toggle"
    >
      {editor.raw ? t('Back to form') : t('Edit TOML')}
    </Button>
  </header>
  {#if editor.layer !== 'global'}
    <p class="layer-help" data-testid="layer-help">
      {editor.layer === 'project'
        ? t('Values set here override Global for this project.')
        : t('Values set here override Global and the project for this repository.')}
    </p>
  {/if}

  {#if settingsStore.pendingRestart.length > 0}
    <div class="banner warn" role="status" data-testid="restart-banner">
      <Icon name="refresh-cw" size={14} />
      {t('Restart Kelta to apply: {keys}', { keys: settingsStore.pendingRestart.join(', ') })}
    </div>
  {/if}

  {#if layerErrors.length > 0}
    <div class="banner err" role="alert" data-testid="layer-issues">
      <Icon name="circle-alert" size={14} />
      <div>
        <strong
          >{t('{file} has errors; the last good configuration is still active.', {
            file: editor.doc?.path.split('/').pop() ?? 'config',
          })}</strong
        >
        {@render issueList(layerErrors)}
      </div>
    </div>
  {/if}
  {#if layerWarnings.length > 0}
    <div class="banner warn" role="status" data-testid="layer-warnings">
      <Icon name="triangle-alert" size={14} />
      <div>
        <strong
          >{t('Warnings: these settings in {file} are ignored; the rest applies.', {
            file: editor.doc?.path.split('/').pop() ?? 'config',
          })}</strong
        >
        {@render issueList(layerWarnings)}
      </div>
    </div>
  {/if}
  {#snippet issueList(list: ValidationIssue[])}
    <ul>
      {#each list as i, n (n)}
        <li>
          {#if i.line}<code>{i.line}:{i.col ?? 1}</code>{/if}
          {#if i.path}<code>{i.path}</code>{/if}
          {i.message}
        </li>
      {/each}
    </ul>
  {/snippet}

  {#if editor.layer === 'repo'}<RepoTrust {editor} />{/if}

  <div class="body">
    <nav aria-label={t('Settings sections')}>
      {#each navGroups as g (g.label)}
        <h2 class="nav-group">{g.label}</h2>
        {#each g.sections as s (s.id)}
          <button
            type="button"
            class:active={sectionId === s.id && !query}
            aria-current={sectionId === s.id && !query ? 'page' : undefined}
            data-section={s.id}
            onclick={() => {
              sectionId = s.id;
              query = '';
              editor.raw = false;
            }}
          >
            <Icon name={s.icon} size={14} />
            {sectionLabel(s)}
          </button>
        {/each}
      {/each}
    </nav>
    <main>
      {#if editor.raw}
        <EditToml {editor} />
      {:else if query.trim()}
        <section data-testid="search-results">
          <h1>{t('Results for “{query}”', { query: query.trim() })}</h1>
          {#if !editor.ready}
            <Spinner size={16} />
          {:else if results.length === 0}
            <EmptyState title={t('No matching setting')} icon="search" />
          {:else}
            {#each results as f (f.path)}
              <div class="result">
                <span class="crumb">{sectionOfRoot(f.segs[0] ?? '')}&ensp;<code>{f.path}</code></span>
                <Field path={f.path} node={f.node} depth={1} />
              </div>
            {/each}
          {/if}
        </section>
      {:else}
        <h1>{section ? sectionLabel(section) : ''}</h1>
        {#if sectionLoading && !Active}
          <Spinner size={16} />
        {:else if Active}
          {@const Section = Active}
          <Section layer={editor.layer} projectId={editor.projectId} repoId={editor.repoId} />
        {/if}
      {/if}
    </main>
  </div>
</div>

<style>
  .layer-help {
    margin: 0;
    padding: var(--k-space-2) var(--k-space-4);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
    background: var(--k-bezel-raised);
  }

  .pane {
    container: settings / inline-size;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--k-well);
  }

  .bar {
    flex-wrap: wrap;
    height: auto;
    min-height: var(--k-toolbar-height);
  }

  /* Same grammar as the tab bar: text, active = 600 + accent underline. */
  .layers {
    display: inline-flex;
    align-self: stretch;
  }

  .layers button {
    padding: 0 var(--k-space-3);
    border: none;
    box-shadow: inset 0 -2px 0 transparent;
    background: transparent;
    color: var(--k-fg-chrome);
    cursor: pointer;
  }

  .layers button.active {
    box-shadow: inset 0 -2px 0 var(--k-accent);
    color: var(--k-fg);
    font-weight: var(--k-weight-strong);
  }

  .layers button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .search {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    margin-left: auto;
    padding: 0 var(--k-space-3);
    height: var(--k-control-height-sm);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
    color: var(--k-fg-subtle);
  }

  .search input {
    border: none;
    background: transparent;
    color: var(--k-fg);
    outline: none;
    min-width: 180px;
  }

  .banner {
    display: flex;
    align-items: flex-start;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-5);
  }

  .banner ul {
    margin: var(--k-space-2) 0 0;
    padding-left: var(--k-space-5);
  }

  .warn {
    background: color-mix(in srgb, var(--k-warn) 12%, transparent);
  }

  .err {
    background: color-mix(in srgb, var(--k-danger) 10%, transparent);
    color: var(--k-fg);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    width: 190px;
    flex: none;
    padding: var(--k-space-2) 0 var(--k-space-5);
    border-right: var(--k-gap) solid var(--k-bezel);
    overflow-y: auto;
  }

  /* Same grammar as list group headers (DESIGN §6.8): 13/600 muted, sentence case. */
  .nav-group {
    margin: var(--k-space-4) 0 var(--k-space-1);
    padding: 0 var(--k-space-4);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-strong);
    color: var(--k-fg-subtle);
  }

  .nav-group:first-child {
    margin-top: var(--k-space-2);
  }

  nav button {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: var(--k-row-height);
    padding: 0 var(--k-space-4);
    border: none;
    background: transparent;
    color: var(--k-fg-muted);
    text-align: left;
    cursor: pointer;
  }

  nav button:hover {
    background: var(--k-bg-hover);
  }

  nav button.active {
    background: var(--k-bg-selected);
    box-shadow: inset 2px 0 0 var(--k-accent);
    color: var(--k-fg);
  }

  main {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: var(--k-space-5) var(--k-space-6);
  }

  main > :global(*) {
    max-width: 880px;
  }

  h1 {
    margin: 0 0 var(--k-space-2);
    font-size: var(--k-font-size-lg);
    font-weight: var(--k-weight-strong);
  }

  /* Narrow pane: a slimmer nav, so the rows keep their two columns longer. */
  @container settings (max-width: 760px) {
    nav {
      width: 150px;
    }

    nav button {
      padding: 0 var(--k-space-3);
    }

    .nav-group {
      padding: 0 var(--k-space-3);
    }

    main {
      padding: var(--k-space-4);
    }
  }

  .result {
    border-bottom: 1px solid var(--k-border);
  }

  .crumb {
    display: block;
    margin-top: var(--k-space-3);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }
</style>
