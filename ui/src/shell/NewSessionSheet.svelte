<script lang="ts">
  import type { SheetProps } from '$app/registry';
  import type { Placement, ProjectId, TemplateCtx } from '$lib/gen';
  import { sessionSpawnTemplate, toolOpen } from '$lib/ipc/commands';
  import { plugins, projects, sessions, settings, toasts, tools, work } from '$lib/stores';
  import { Button, Select, Sheet, TextInput } from '$lib/ui';

  import { activateProject, focusedSessionId, railProjects } from './nav';

  let { onclose }: SheetProps = $props();

  const BUILTIN = [
    { value: 'shell', label: 'Shell' },
    { value: 'claude', label: 'Claude' },
    { value: 'editor', label: 'nvim' },
    { value: 'claude+editor', label: 'Claude + nvim' },
  ];

  const rail = railProjects();
  let projectId = $state<ProjectId>(projects.activeId ?? rail[0]?.id ?? 'home');
  let template = $state('shell');
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
      ...tools.list(projectId).map((t) => ({
        value: `tool:${t.id}`,
        label: `Tool: ${t.label}${t.installed === false ? ' (not installed)' : ''}`,
        disabled: t.installed === false,
      })),
    ];
  });

  const cwdOptions = $derived.by(() => {
    const out: { value: string; label: string }[] = [{ value: '', label: 'Project default' }];
    const push = (value: string, label: string): void => {
      if (!out.some((o) => o.value === value)) out.push({ value, label });
    };
    const project = projects.byId(projectId);
    for (const r of project?.repos ?? []) push(r.path, `${r.id} — ${r.path}`);
    for (const w of work.forProject(projectId)) {
      if (w.state.kind !== 'finished') push(w.worktree, `worktree ${w.branch} — ${w.worktree}`);
    }
    for (const s of sessions.forProject(projectId)) push(s.cwd, `recent — ${s.cwd}`);
    out.push({ value: '__custom', label: 'Other directory…' });
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
      if (projects.activeId !== projectId) await activateProject(projectId);
    } catch (err) {
      toasts.error(err, 'Starting the session failed');
    } finally {
      busy = false;
    }
  }
</script>

<Sheet title="New session" width={480} {onclose}>
  <form
    class="form"
    data-testid="new-session"
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <Select label="Template" options={templateOptions} bind:value={template} />
    <Select label="Project" options={projectOptions} bind:value={projectId} />
    <Select label="Directory" options={cwdOptions} bind:value={cwdChoice} />
    {#if cwdChoice === '__custom'}
      <TextInput label="Directory path" bind:value={custom} placeholder="/path/to/dir" />
    {/if}
    <Select
      label="Placement"
      options={[
        { value: 'new_tab', label: 'New tab' },
        { value: 'split_right', label: 'Split right' },
        { value: 'split_down', label: 'Split down' },
      ]}
      bind:value={placement}
    />
    <button type="submit" class="k-visually-hidden" tabindex="-1">Start</button>
  </form>
  {#snippet actions()}
    <Button onclick={onclose}>Cancel</Button>
    <Button variant="primary" icon="play" loading={busy} onclick={start} data-testid="new-session-start"
      >Start</Button
    >
  {/snippet}
</Sheet>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
  }
</style>
