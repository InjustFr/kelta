<script lang="ts">
  // Settings pane (Mod+,): layer selector, section list, search across every owned key, the
  // schema-generated forms and a raw "Edit TOML" mode per layer.
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
  import { leafFields, descriptionOf, titleOf, enumOptions, type LeafField } from './lib/schema';
  import { SECTION_ROOTS, sectionLabel } from './lib/sections';
  import RepoTrust from './RepoTrust.svelte';

  let { projectId, content }: PaneProps<'settings'> = $props();

  const tomlProject = takeTomlRequest();
  const initialTarget = () => ({
    layer: tomlProject ? ('project' as const) : ('global' as const),
    projectId: tomlProject ?? (projectId !== 'home' ? projectId : null),
  });
  const editor = provideEditor(new SettingsEditor(initialTarget()));
  const platform = currentPlatform();
  const sections = $derived(settingsSections.filter((s) => !s.platform || s.platform === platform));

  let sectionId = $state<string>('general');
  let query = $state('');
  let editRaw = $state(tomlProject !== null);
  let Active = $state<Component<SettingsSectionProps> | null>(null);
  let sectionLoading = $state(false);
  let layerIssues = $state<ValidationIssue[]>([]);

  const configurable = $derived(projects.list.filter((p) => !p.builtin));
  const projectOptions = $derived(configurable.map((p) => ({ value: p.id, label: p.name })));
  const currentProject = $derived(projects.byId(editor.projectId ?? ''));
  const repoOptions = $derived((currentProject?.repos ?? []).map((r) => ({ value: r.id, label: r.id })));

  $effect(() => {
    // initial section from the pane content (e.g. "Open Accounts settings")
    const wanted = content.section;
    if (wanted && sections.some((s) => s.id === wanted)) sectionId = wanted;
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
    sections.find((s) => SECTION_ROOTS[s.id]?.includes(root))?.label ?? '';

  function matches(f: LeafField, q: string): boolean {
    const hay = [
      f.path,
      titleOf(f.segs[f.segs.length - 1] ?? ''),
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
    <div class="layers" role="group" aria-label="Layer being edited">
      <button
        type="button"
        class:active={editor.layer === 'global'}
        aria-pressed={editor.layer === 'global'}
        onclick={() => chooseLayer('global')}
      >
        Global
      </button>
      <button
        type="button"
        class:active={editor.layer === 'project'}
        aria-pressed={editor.layer === 'project'}
        disabled={configurable.length === 0}
        onclick={() => chooseLayer('project')}
      >
        Project
      </button>
      <button
        type="button"
        class:active={editor.layer === 'repo'}
        aria-pressed={editor.layer === 'repo'}
        disabled={configurable.length === 0}
        onclick={() => chooseLayer('repo')}
      >
        Repo
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
      <input type="search" placeholder="Search settings" aria-label="Search settings" bind:value={query} />
    </label>
    <Button
      size="sm"
      icon="file-code"
      variant={editRaw ? 'primary' : 'ghost'}
      onclick={() => (editRaw = !editRaw)}
      data-testid="edit-toml-toggle"
    >
      {editRaw ? 'Back to form' : 'Edit TOML'}
    </Button>
  </header>
  <p class="layer-help">
    Global applies everywhere. Project and Repo override it for one project or one repository.
  </p>

  {#if settingsStore.pendingRestart.length > 0}
    <div class="banner warn" role="status" data-testid="restart-banner">
      <Icon name="refresh-cw" size={14} />
      Restart Kelta to apply: {settingsStore.pendingRestart.join(', ')}
    </div>
  {/if}

  {#if layerIssues.length > 0}
    <div class="banner err" role="alert" data-testid="layer-issues">
      <Icon name="circle-alert" size={14} />
      <div>
        <strong
          >{editor.doc?.path.split('/').pop() ?? 'config'} has errors; the last good configuration is still active.</strong
        >
        <ul>
          {#each layerIssues as i, n (n)}
            <li>
              {#if i.line}<code>{i.line}:{i.col ?? 1}</code>{/if}
              {#if i.path}<code>{i.path}</code>{/if}
              {i.message}
            </li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}

  {#if editor.layer === 'repo'}<RepoTrust {editor} />{/if}

  <div class="body">
    <nav aria-label="Settings sections">
      {#each sections as s (s.id)}
        <button
          type="button"
          class:active={sectionId === s.id && !query}
          aria-current={sectionId === s.id && !query ? 'page' : undefined}
          data-section={s.id}
          onclick={() => {
            sectionId = s.id;
            query = '';
            editRaw = false;
          }}
        >
          <Icon name={s.icon} size={14} />
          {sectionLabel(s)}
        </button>
      {/each}
    </nav>
    <main>
      {#if editRaw}
        <EditToml {editor} />
      {:else if query.trim()}
        <section data-testid="search-results">
          <h2>Results for “{query.trim()}”</h2>
          {#if !editor.ready}
            <Spinner size={16} />
          {:else if results.length === 0}
            <EmptyState title="No matching setting" icon="search" />
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
        <h2>{section ? sectionLabel(section) : ''}</h2>
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
    border-bottom: 1px solid var(--k-border);
  }

  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--k-well);
  }

  .bar {
    flex-wrap: wrap;
    height: auto;
    min-height: var(--k-tabbar-height);
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
    padding: var(--k-space-3) 0;
    border-right: var(--k-gap) solid var(--k-bezel);
    overflow-y: auto;
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

  h2 {
    margin: 0 0 var(--k-space-3);
    font-size: var(--k-font-size-lg);
    font-weight: var(--k-weight-strong);
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
