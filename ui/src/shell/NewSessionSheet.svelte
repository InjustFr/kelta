<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SheetProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { Placement, ProjectId, TemplateCtx } from '$lib/gen';
  import { sessionSpawnTemplate, toolOpen } from '$lib/ipc/commands';
  import { effectiveChords } from '$lib/keys/manager';
  import { plugins, projects, sessions, settings, toasts, tools, work } from '$lib/stores';
  import { Button, currentPlatform, Kbd, Select, Sheet, TextInput } from '$lib/ui';

  import BrowseButton from './BrowseButton.svelte';
  import { jumplist } from './jumplist';
  import { activateProject, currentTab, focusedSessionId, isToolTab, railProjects } from './nav';

  let { onclose }: SheetProps = $props();

  const BUILTIN = $derived([
    { value: 'claude+editor', label: t('Claude and nvim (recommended)') },
    { value: 'claude', label: 'Claude Code' },
    { value: 'editor', label: 'nvim' },
    { value: 'shell', label: t('Shell') },
  ]);

  const rail = railProjects();
  const newWorkChord = effectiveChords('work.new', settings.value()?.keys ?? null, currentPlatform())[0];
  let projectId = $state<ProjectId>(initialProject());

  /** The active project; in a Home tool tab, the project of the last row shown (Home is no target). */
  function initialProject(): ProjectId {
    const active = projects.activeId;
    const tab = currentTab();
    if (active && active === projects.home?.id && tab && isToolTab(tab)) {
      const last = jumplist.list
        .slice(0, jumplist.at + 1)
        .reverse()
        .find((l) => l.project !== active && projects.byId(l.project)?.open);
      if (last) return last.project;
    }
    return active ?? rail[0]?.id ?? 'home';
  }
  let picked = $state<string | null>(null);
  let placement = $state<Placement>('new_tab');
  let cwdChoice = $state('');
  let custom = $state('');
  let busy = $state(false);

  const projectOptions = $derived(
    [...rail]
      .sort((a, b) => Number(b.id === projects.activeId) - Number(a.id === projects.activeId))
      .map((p) => ({
        value: p.id,
        label: p.name,
      })),
  );

  $effect(() => {
    if (tools.byProject[projectId] === undefined) void tools.load(projectId);
  });

  const templateOptions = $derived.by(() => {
    const configured = settings.value()?.session_templates.filter((t) => t.enabled) ?? [];
    const fromPlugins = (plugins.plugins.data ?? [])
      .filter((p) => p.enabled)
      .flatMap((p) =>
        p.contributes.session_templates.filter((t) => t.enabled).map((t) => ({ ...t, plugin: p.name })),
      );
    const base = configured.length > 0 ? configured.map((t) => ({ value: t.id, label: t.label })) : BUILTIN;
    return [
      ...base,
      ...fromPlugins.map((t) => ({ value: t.id, label: `${t.label} (${t.plugin})` })),
      ...tools.list(projectId).map((tool) => ({
        value: `tool:${tool.id}`,
        label:
          tool.installed === false
            ? t('Tool: {tool} (not installed)', { tool: tool.label })
            : t('Tool: {tool}', { tool: tool.label }),
        disabled: tool.installed === false,
      })),
    ];
  });

  // Until the user picks one: Claude and nvim when offered, otherwise the first option.
  const template = $derived(
    picked ??
      (templateOptions.some((o) => o.value === 'claude+editor')
        ? 'claude+editor'
        : (templateOptions[0]?.value ?? '')),
  );

  const cwdOptions = $derived.by(() => {
    const out: { value: string; label: string }[] = [{ value: '', label: t('Project folder') }];
    const push = (value: string, label: string): void => {
      if (!out.some((o) => o.value === value)) out.push({ value, label });
    };
    const project = projects.byId(projectId);
    for (const r of project?.repos ?? []) push(r.path, `${r.id} (${r.path})`);
    for (const w of work.forProject(projectId)) {
      if (w.state.kind !== 'finished')
        push(
          w.worktree,
          t('Branch {branch} (separate folder): {path}', { branch: w.branch, path: w.worktree }),
        );
    }
    for (const s of sessions.forProject(projectId)) push(s.cwd, t('Recent: {path}', { path: s.cwd }));
    out.push({ value: '__custom', label: t('Other folder…') });
    return out;
  });

  const cwd = $derived(cwdChoice === '__custom' ? custom.trim() : cwdChoice);

  $effect(() => {
    // The chosen directory belongs to the chosen project.
    void projectId;
    cwdChoice = '';
  });

  function context(): TemplateCtx {
    const repo = projects
      .byId(projectId)
      ?.repos.find(
        (r) => cwd !== '' && (cwd === r.path || cwd.startsWith(r.path.endsWith('/') ? r.path : r.path + '/')),
      );
    return {
      repo_id: repo?.id ?? null,
      cwd: cwd === '' ? null : cwd,
      session_id: focusedSessionId(),
      work_item_id: null,
      ticket: null,
      review: null,
      extra: {},
    };
  }

  async function start(): Promise<void> {
    busy = true;
    try {
      if (template.startsWith('tool:')) {
        await toolOpen({ project_id: projectId, tool_id: template.slice(5), ctx: context(), placement });
      } else {
        const created = await sessionSpawnTemplate({
          project_id: projectId,
          template_id: template,
          ctx: context(),
          placement,
        });
        for (const s of created) sessions.upsert(s);
      }
      onclose();
      await activateProject(projectId);
    } catch (err) {
      toasts.error(err, t('Starting the session failed'));
    } finally {
      busy = false;
    }
  }
</script>

<Sheet title={t('New session')} width={480} {onclose}>
  <form
    class="form"
    data-testid="new-session"
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <Select
      label={t('What to start')}
      options={templateOptions}
      bind:value={() => template, (v) => (picked = v)}
    />
    <Select label={t('Project')} options={projectOptions} bind:value={projectId} />
    <Select label={t('Folder')} options={cwdOptions} bind:value={cwdChoice} />
    {#if cwdChoice === '__custom'}
      <div class="pick">
        <TextInput label={t('Folder path')} bind:value={custom} placeholder={t('/path/to/dir')} />
        <BrowseButton value={custom} onpick={(p) => (custom = p)} />
      </div>
    {/if}
    <Select
      label={t('Open in')}
      options={[
        { value: 'new_tab', label: t('New tab') },
        { value: 'split_right', label: t('Split right') },
        { value: 'split_down', label: t('Split down') },
      ]}
      bind:value={placement}
    />
    <button type="submit" class="k-visually-hidden" tabindex="-1">{t('Start')}</button>
  </form>
  <p class="hint" data-testid="new-work-hint">
    {t('Want a branch and a PR?')}
    <button
      type="button"
      class="k-link"
      onclick={() => {
        onclose();
        void dispatch('work.new');
      }}>{t('New work item')}</button
    >
    {#if newWorkChord}<Kbd chord={newWorkChord} />{/if}
  </p>
  {#snippet actions()}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button variant="primary" icon="play" loading={busy} onclick={start} data-testid="new-session-start"
      >{t('Start')}</Button
    >
  {/snippet}
</Sheet>

<style>
  .hint {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: var(--k-space-5) 0 0;
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-sm);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
  }

  .pick {
    display: flex;
    align-items: flex-end;
    gap: var(--k-space-3);
  }

  .pick :global(.k-field) {
    flex: 1;
  }
</style>
