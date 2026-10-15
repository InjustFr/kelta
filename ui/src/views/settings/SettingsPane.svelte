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
  import { dispatch } from '$lib/actions';
  import * as ipc from '$lib/ipc/commands';
  import { projects, settings as settingsStore, toasts } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Kbd from '$lib/ui/Kbd.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { currentPlatform } from '$lib/ui/format';
  import { tick, untrack, type Component } from 'svelte';

  import { noteRecent } from '../../shell/jumplist';
  import CountChips from '../filters/CountChips.svelte';

  import { takeChangedRequest, takeKeyRequest, takeTomlRequest } from './actions';
  import EditToml from './EditToml.svelte';
  import Field from './fields/Field.svelte';
  import {
    provideEditor,
    readLayerFiles,
    SettingsEditor,
    type EditLayer,
    type LayerFile,
  } from './lib/editor.svelte';
  import { effectiveBindings } from './lib/chords';
  import { isRecord } from './lib/paths';
  import { repoRelevant } from './lib/schema';
  import {
    buildSearchIndex,
    changedSettings,
    isInline,
    resettable,
    searchSettings,
    type ChangedRow,
    type SearchEntry,
    type SearchHit,
  } from './lib/search';
  import { SECTION_GROUPS, sectionLabel } from './lib/sections';
  import { health } from './lib/health.svelte';
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
  /** settings.open { key } (⌘K #478, a feature's link #477): opened once the search index is built. */
  let pendingKey = $state<string | null>(null);
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
    const key = takeKeyRequest();
    if (key) pendingKey = key;
    if (takeChangedRequest()) {
      changedOn = true;
      query = '';
      active = 0;
      editor.raw = false;
    }
    const toml = takeTomlRequest();
    if (toml) {
      editor.raw = true;
      untrack(() => void editor.target({ layer: 'project', projectId: toml, repoId: null }));
    }
  });

  // A section shown is a place ⌘K lists under Recent.
  $effect(() => noteRecent({ id: `setting:${sectionId}`, label: sectionId }));

  $effect(() => {
    // initial load only: layer / project / repo changes reload through editor.target()
    untrack(() => {
      void editor.load();
      void health.refresh(); // the Health nav count
    });
  });
  const problemCount = $derived(health.problems.length);

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

  // ---- search (#469) ------------------------------------------------------------------------------
  const SHOWN = 12;
  let listEl = $state<HTMLElement>();
  let mainEl = $state<HTMLElement>();
  let active = $state(0);
  let allFor = $state<string | null>(null); // the query "Show all" was pressed for
  const bindings = $derived.by(() => {
    const v = editor.valueOf('keys.bindings');
    return effectiveBindings(isRecord(v) ? (v as Record<string, string[]>) : {}, platform);
  });
  const index = $derived(editor.schema ? buildSearchIndex(editor.schema, sections, bindings) : []);
  $effect(() => {
    // A cold open has no schema yet: the row opens once it is loaded (#477).
    const key = pendingKey;
    if (!key || index.length === 0) return;
    pendingKey = null;
    const entry = index.find((e) => e.kind === 'key' && e.path === key);
    untrack(() => {
      if (entry) open(entry);
      else {
        query = key; // a key the forms do not show: the search says so
        active = 0;
      }
    });
  });
  const searching = $derived(query.trim() !== '' && !editor.raw);
  const hits = $derived(
    searching
      ? searchSettings(index, query).filter(
          (h) => h.kind !== 'key' || editor.layer !== 'repo' || repoRelevant(h.path!),
        )
      : [],
  );
  const shown = $derived(allFor === query ? hits : hits.slice(0, SHOWN));
  // ---- Changed by you (#474) ----------------------------------------------------------------------
  let changedOn = $state(false);
  let files = $state<LayerFile[]>([]);
  $effect(() => {
    void settingsStore.lastChange; // any write (form, TOML, ⌘K, another window) re-reads the files
    const list = configurable;
    void readLayerFiles(list).then((f) => (files = f));
  });
  const changedRows = $derived(changedSettings(index, files));
  const changedHits = $derived(query.trim() ? searchSettings(changedRows, query) : changedRows);
  const counts = $derived(
    (changedOn ? changedHits : hits).reduce<Record<string, number>>(
      (m, h) => ((m[h.section] = (m[h.section] ?? 0) + 1), m),
      {},
    ),
  );
  $effect(() => {
    void active;
    listEl?.querySelector('.result.active')?.scrollIntoView?.({ block: 'nearest' });
  });

  let stopReveal: (() => void) | null = null;
  /** Scrolls to `selector` in the section once it is drawn (the section loads, then its data); event-driven, no timer. */
  function reveal(selector: string, then?: (el: HTMLElement) => void): void {
    stopReveal?.();
    const found = (): boolean => {
      const el = mainEl?.querySelector<HTMLElement>(selector);
      if (!el) return false;
      el.closest('details')?.setAttribute('open', ''); // an Advanced key
      el.scrollIntoView?.({ block: 'center' });
      el.removeAttribute('data-revealed');
      void el.offsetWidth; // restarts the highlight on a row revealed before
      el.setAttribute('data-revealed', '');
      then?.(el);
      return true;
    };
    if (!mainEl || found()) return;
    const obs = new MutationObserver(() => {
      if (found()) stopReveal?.();
    });
    obs.observe(mainEl, { childList: true, subtree: true });
    stopReveal = () => {
      obs.disconnect();
      stopReveal = null;
    };
  }

  function open(h: SearchEntry): void {
    changedOn = false;
    query = '';
    sectionId = h.section;
    editor.raw = !!h.toml; // TOML-only keys open Edit TOML
    if (h.toml) return;
    const sel =
      h.kind === 'key'
        ? `[data-path="${CSS.escape(h.path!)}"]`
        : h.kind === 'action'
          ? `[data-action="${CSS.escape(h.path!)}"]`
          : h.anchor
            ? `[data-testid="${h.anchor}"]`
            : null;
    const record =
      h.kind === 'action'
        ? (el: HTMLElement) => el.querySelector<HTMLElement>('[data-testid="record"]')?.click()
        : undefined;
    // after tick: the results (which hold the same data-path) are gone
    if (sel) void tick().then(() => reveal(sel, record));
  }

  /** A Changed by you row: the form on that row's file, then the row (#474). */
  function openChanged(r: ChangedRow): void {
    void editor.target(r.file);
    open(r);
  }

  /** Removes the row's key from its file through the form's write path (as #478's toggleSetting). */
  async function resetChanged(r: ChangedRow): Promise<void> {
    const ed = new SettingsEditor(r.file);
    await ed.refreshDoc(); // a repo file's trust
    for (const p of r.resets)
      if (!(await ed.reset(p)))
        toasts.error(ed.errors[p] ?? '', t('Could not reset {name}', { name: r.label }));
    await editor.refreshDoc();
    files = await readLayerFiles(configurable);
  }

  /** ↵ on a hit: a simple value focuses its control, anything else opens. */
  function activate(h: SearchHit): void {
    if (!isInline(h)) return open(h);
    const row = listEl?.querySelector(`[data-hit="${CSS.escape(h.id)}"]`);
    (
      row?.querySelector<HTMLElement>('input, select, textarea') ?? row?.querySelector<HTMLElement>('button')
    )?.focus();
  }

  function onSearchKey(e: KeyboardEvent): void {
    if (e.key === 'Escape' && query)
      query = ''; // empty: GlobalHost closes Settings (data-esc-close)
    else if (e.key === 'ArrowDown')
      active = Math.min(active + 1, (changedOn ? changedHits : shown).length - 1);
    else if (e.key === 'ArrowUp') active = Math.max(active - 1, 0);
    else if (e.key === 'Enter' && changedOn) {
      if (changedHits[active]) openChanged(changedHits[active]);
    } else if (e.key === 'Enter' && shown[active]) activate(shown[active]);
    else return;
    e.preventDefault();
  }
</script>

<div class="pane" data-testid="settings-pane" data-pane-kind="settings">
  <header class="k-toolbar bar">
    <label class="search">
      <Icon name="search" size={14} />
      <!-- svelte-ignore a11y_autofocus -->
      <input
        type="search"
        placeholder={t('Search settings')}
        aria-label={t('Search settings')}
        bind:value={query}
        autofocus
        data-esc-close
        oninput={() => (active = 0)}
        onkeydown={onSearchKey}
      />
    </label>
    <CountChips
      label={t('Changed by you')}
      chips={[
        {
          id: 'changed',
          label: t('Changed by you'),
          title: t('Everything that differs from Kelta’s defaults, and where it is set.'),
          n: changedRows.length,
        },
      ]}
      value={changedOn ? 'changed' : null}
      onchange={(id) => {
        changedOn = id !== null;
        active = 0;
        if (changedOn) editor.raw = false;
      }}
    />
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
            class:dim={(searching || changedOn) && !counts[s.id]}
            class:active={sectionId === s.id && !query}
            aria-current={sectionId === s.id && !query ? 'page' : undefined}
            data-section={s.id}
            onclick={() => {
              stopReveal?.();
              sectionId = s.id;
              query = '';
              changedOn = false;
              editor.raw = false;
            }}
          >
            <Icon name={s.icon} size={14} />
            {sectionLabel(s)}
            {#if searching || changedOn}
              {#if counts[s.id]}<span class="count k-num" data-testid="nav-hits">{counts[s.id]}</span>{/if}
            {:else if s.id === 'health' && problemCount > 0}
              <span class="count" data-testid="health-count"><Badge tone="danger">{problemCount}</Badge></span
              >
            {/if}
          </button>
        {/each}
      {/each}
    </nav>
    <main bind:this={mainEl}>
      {#if editor.raw}
        <EditToml {editor} />
      {:else if changedOn}
        <section data-testid="changed-settings">
          <h1>{t('Changed by you')}</h1>
          <p class="help">{t('Everything that differs from Kelta’s defaults, and where it is set.')}</p>
          {#if changedHits.length === 0}
            <EmptyState
              icon="search"
              title={query.trim()
                ? t('No matching setting')
                : t('Nothing changed: every setting is at its default.')}
            />
          {:else}
            <ol class="results" bind:this={listEl}>
              {#each changedHits as r, i (r.id)}
                <li class="result" class:active={i === active} data-hit={r.id}>
                  <span class="crumb"
                    >{r.crumb}{#if r.path}&ensp;<code>{r.path}</code>{/if}</span
                  >
                  <div class="line">
                    <span class="label">{r.label}</span>
                    {#if isInline(r) && r.field !== 'secret'}<code>{String(r.value)}</code>{/if}
                    {#each r.chords ?? [] as c (c)}<Kbd chord={c} />{/each}
                    <Badge tone="accent">{r.file.where}</Badge>
                    <Button size="sm" variant="ghost" onclick={() => openChanged(r)}
                      >{r.toml ? t('Edit TOML') : t('Open')}</Button
                    >
                    {#if resettable(r)}
                      <IconButton
                        icon="rotate-ccw-clock"
                        size="sm"
                        label={t('Reset {name} in {file}', { name: r.label, file: r.file.where })}
                        data-testid="changed-reset"
                        onclick={() => resetChanged(r)}
                      />
                    {/if}
                  </div>
                </li>
              {/each}
            </ol>
          {/if}
        </section>
      {:else if searching}
        <section data-testid="search-results">
          <h1>{t('Results for “{query}”', { query: query.trim() })}</h1>
          {#if !editor.ready}
            <Spinner size={16} />
          {:else if hits.length === 0}
            <EmptyState title={t('No matching setting')} icon="search">
              {#snippet actions()}
                <Button size="sm" onclick={() => dispatch('palette.open')}
                  >{t('Search actions in {keys}', { keys: platform === 'macos' ? '⌘K' : 'Ctrl+K' })}</Button
                >
                <Button
                  size="sm"
                  variant="ghost"
                  icon="file-code"
                  onclick={() => {
                    query = '';
                    editor.raw = true;
                  }}>{t('Open the TOML file')}</Button
                >
              {/snippet}
            </EmptyState>
          {:else}
            <ol class="results" bind:this={listEl}>
              {#each shown as h, i (h.id)}
                <li class="result" class:active={i === active} data-hit={h.id}>
                  <span class="crumb"
                    >{h.crumb}{#if h.path}&ensp;<code>{h.path}</code>{/if}{#if h.why}<span class="why"
                        >{h.why}</span
                      >{/if}</span
                  >
                  {#if isInline(h)}
                    <Field path={h.path!} node={h.node!} depth={1} />
                  {:else}
                    <div class="line">
                      <span class="label">{h.label}</span>
                      {#each h.chords ?? [] as c (c)}<Kbd chord={c} />{/each}
                      <Button size="sm" variant="ghost" onclick={() => open(h)}
                        >{h.kind === 'action' ? t('Record') : h.toml ? t('Edit TOML') : t('Open')}</Button
                      >
                    </div>
                    {#if h.kind === 'section' && h.help}<p class="help">{h.help}</p>{/if}
                  {/if}
                </li>
              {/each}
            </ol>
            {#if shown.length < hits.length}
              <Button size="sm" variant="ghost" onclick={() => (allFor = query)}
                >{t('Show all {n} results', { n: hits.length })}</Button
              >
            {/if}
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
  /* The row ⌘K or a search result opened: a short accent wash. */
  main :global([data-revealed]) {
    animation: k-revealed 1.6s var(--k-ease);
  }

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
    /* room for the fr/de placeholder (#362) */
    width: 30ch;
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

  /* Rows keep their height and the list scrolls: shrinking them cut sections off (#362). */
  nav > * {
    flex: none;
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
    min-height: var(--k-row-height);
    padding: var(--k-space-1) var(--k-space-4);
    border: none;
    background: transparent;
    color: var(--k-fg-muted);
    text-align: left;
    cursor: pointer;
  }

  nav .count {
    margin-left: auto;
  }

  nav button.dim {
    opacity: 0.5;
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
      padding: var(--k-space-1) var(--k-space-3);
    }

    .nav-group {
      padding: 0 var(--k-space-3);
    }

    main {
      padding: var(--k-space-4);
    }

    .search input {
      width: 180px;
    }
  }

  .results {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .result {
    border-bottom: 1px solid var(--k-border);
  }

  .result.active {
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    min-height: var(--k-row-height);
  }

  .line .label {
    flex: 1;
  }

  .why {
    margin-left: var(--k-space-2);
    color: var(--k-fg-muted);
    font-style: italic;
  }

  .help {
    margin: 0 0 var(--k-space-2);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .crumb {
    display: block;
    margin-top: var(--k-space-3);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }
</style>
