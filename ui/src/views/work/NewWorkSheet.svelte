<script lang="ts">
  // New work item (FLOW §4.3): a task, a wip/ branch in its own worktree, Claude started on the task.
  import type { SheetProps } from '$app/registry';
  import type { ProjectId, TicketItem } from '$lib/gen';
  import { workLink, workPlan, workStart } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, settings, toasts, work } from '$lib/stores';
  import { Button, currentPlatform, IconButton, Kbd, Select, Sheet, TextInput } from '$lib/ui';

  import { activateProject, railProjects } from '../../shell/nav';
  import { scratchBranch, taskTitle, validateBranch } from './common';
  import TicketPicker from './TicketPicker.svelte';

  let { onclose }: SheetProps = $props();

  const withRepos = railProjects().filter((p) => p.repos.length > 0);
  const startId = withRepos.find((p) => p.id === projects.activeId)?.id ?? withRepos[0]?.id ?? '';

  let task = $state('');
  let projectId = $state<ProjectId>(startId);
  let layoutId = $state(settings.value()?.work.default_template ?? 'claude+editor');
  let branchEdit = $state<string | null>(null);
  let ticket = $state<TicketItem | null>(null);
  let picking = $state(false);
  let touched = $state(false);
  let busy = $state(false);
  let failure = $state<string | null>(null);

  const project = $derived(projects.byId(projectId));
  const repos = $derived(project?.repos ?? []);

  // A project change picks its primary repo; a repo change picks its base. Both stay editable.
  let repoId = $derived((repos.find((x) => x.primary) ?? repos[0])?.id ?? '');
  let base = $derived(repos.find((r) => r.id === repoId)?.base ?? 'main');

  const s = $derived(settings.value());
  const auto = $derived(
    scratchBranch(task, s?.work.scratch_branch_template ?? 'wip/{slug}', s?.worktree.slug_max ?? 40),
  );
  const branch = $derived(branchEdit ?? auto);
  const title = $derived(taskTitle(task));
  const layouts = $derived.by(() => {
    const list = (s?.session_templates ?? [])
      .filter((t) => t.enabled)
      .map((t) => ({ value: t.id, label: t.label }));
    return list.some((t) => t.value === layoutId) ? list : [{ value: layoutId, label: layoutId }, ...list];
  });

  const errors = $derived({
    task: title === '' ? 'Describe what Claude should do' : null,
    branch: branch === '' && branchEdit === null ? null : validateBranch(branch),
    base: base.trim() === '' ? 'Base branch is required' : null,
    project: project ? null : 'Open a project with a repository first',
  });
  const valid = $derived(Object.values(errors).every((e) => e === null));

  async function submit(): Promise<void> {
    touched = true;
    if (!valid || busy) return;
    busy = true;
    failure = null;
    try {
      // An untouched branch is left to the backend so it renders the real template.
      const plan = await workPlan({
        project_id: projectId,
        source: { kind: 'branch', name: branchEdit ?? '', task, repo: repoId || null },
      });
      plan.base = base.trim();
      plan.template_id = layoutId;
      let item = await workStart({ plan });
      work.upsert(item);
      if (ticket && item.state.kind !== 'failed') {
        try {
          item = await workLink({ id: item.id, ticket: ticket.ticket.ref, apply_side_effects: true });
          work.upsert(item);
        } catch (err) {
          toasts.error(err, `Linking ${ticket.ticket.ref.key}`);
        }
      }
      if (item.state.kind === 'failed') toasts.warn(`${item.branch}: start failed at ${item.state.step}`);
      else toasts.info(`Started ${item.branch}`);
      onclose();
      await activateProject(projectId);
    } catch (err) {
      failure = toIpcError('work_start', err).message;
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      e.stopPropagation();
      void submit();
    }
  }
</script>

<Sheet title="New work item" width={480} {onclose}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    id="new-work-form"
    class="form"
    data-testid="new-work"
    onsubmit={(e) => {
      e.preventDefault();
      void submit();
    }}
    {onkeydown}
  >
    <p class="lede">
      Its own branch in a separate folder, with Claude ready to start. You can link a ticket later.
    </p>
    <TextInput
      label="What should Claude do?"
      bind:value={task}
      multiline
      rows={6}
      autofocus
      error={touched ? errors.task : null}
    />
    <div class="grid">
      <Select
        label="Project"
        options={withRepos.map((p) => ({ value: p.id, label: p.name }))}
        bind:value={projectId}
      />
      {#if repos.length > 1}
        <Select label="Repo" options={repos.map((r) => ({ value: r.id, label: r.id }))} bind:value={repoId} />
      {/if}
      <TextInput
        label="Branch"
        class="mono"
        value={branch}
        oninput={(e) => (branchEdit = e.currentTarget.value)}
        placeholder="wip/…"
        hint={branchEdit === null ? 'From the first line' : undefined}
        error={errors.branch}
        spellcheck={false}
        data-testid="new-work-branch"
      />
      <TextInput label="Base" class="mono" bind:value={base} error={touched ? errors.base : null} />
      <Select label="Layout" options={layouts} bind:value={layoutId} />
      <div class="ticket">
        <span class="label">Ticket</span>
        {#if ticket}
          <span class="picked" data-testid="new-work-ticket">
            <code>{ticket.ticket.ref.key}</code>
            <span class="ttl">{ticket.ticket.title}</span>
            <IconButton icon="x" label="Remove ticket" size="sm" onclick={() => (ticket = null)} />
          </span>
          <span class="hint">Assigned to you and moved per work.on_start; the branch stays.</span>
        {:else}
          <Button size="sm" variant="ghost" icon="ticket" onclick={() => (picking = true)}
            >None. Link one…</Button
          >
        {/if}
      </div>
    </div>
    {#if touched && errors.project}<p class="fail" role="alert">{errors.project}</p>{/if}
    {#if failure}<p class="fail" role="alert" data-testid="new-work-error">{failure}</p>{/if}
  </form>
  {#snippet actions()}
    {#if title}<span class="note" {title}>Title: “{title}”</span>{/if}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button variant="primary" type="submit" form="new-work-form" loading={busy} disabled={touched && !valid}>
      Start <Kbd chord={currentPlatform() === 'macos' ? 'cmd+enter' : 'ctrl+enter'} />
    </Button>
  {/snippet}
</Sheet>

{#if picking}
  <TicketPicker
    title="Link a ticket"
    onpick={(hit) => {
      ticket = hit;
      picking = false;
    }}
    onclose={() => (picking = false)}
  />
{/if}

<style>
  .form,
  .grid {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .lede {
    margin: 0;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .form :global(.mono) {
    font-family: var(--k-font-mono);
  }

  .ticket {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-1);
  }

  .label {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .picked {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    max-width: 100%;
  }

  .ttl {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .note {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--k-fg-subtle);
    font-size: var(--k-font-size-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fail {
    margin: 0;
    padding: var(--k-space-2) var(--k-space-3);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    color: var(--k-danger);
  }
</style>
