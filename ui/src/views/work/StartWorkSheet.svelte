<script lang="ts">
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type {
    BranchChoice,
    ClaudeEffort,
    KeltaError,
    PermissionMode,
    StartWorkPlan,
    TransitionTarget,
    WorkItem,
  } from '$lib/gen';
  import { openExternal, workResume, workRetryStep, workStart } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { tickets, toasts, work } from '$lib/stores';
  import { Button, Icon, Select, Sheet, Spinner, TextInput, Toggle } from '$lib/ui';

  import { planValid, START_STEPS, stepLabel, validatePlan } from './common';

  interface Props extends SheetProps {
    plan: StartWorkPlan;
  }

  let { onclose, plan: initial }: Props = $props();

  const seed = untrack(() => structuredClone($state.snapshot(initial)) as StartWorkPlan);
  let plan = $state<StartWorkPlan>(seed);
  const origTransition: TransitionTarget | null = seed.side_effects.transition_to;
  let transitionOn = $state(origTransition !== null);
  let commentOn = $state(seed.side_effects.comment !== null);
  let commentText = $state(seed.side_effects.comment ?? '');
  let touched = $state(false);

  type Phase = 'edit' | 'running' | 'failed';
  let phase = $state<Phase>('edit');
  let startedAt = 0;
  let startedItem = $state<WorkItem | null>(null);
  let failure = $state<KeltaError | null>(null);
  let retrying = $state<string | null>(null);

  const resuming = $derived(seed.existing !== null);
  const isTicket = $derived(plan.source.kind === 'ticket');
  const subject = $derived(
    plan.source.kind === 'ticket'
      ? plan.source.ticket.key
      : plan.source.kind === 'review'
        ? `${plan.source.review.repo}#${plan.source.review.number}`
        : plan.source.name,
  );
  const title = $derived(
    resuming
      ? `Resume ${subject}`
      : plan.source.kind === 'review'
        ? `Review locally ${subject}`
        : plan.source.kind === 'branch'
          ? `New branch workspace`
          : `Start work on ${subject}`,
  );

  const built = $derived<StartWorkPlan>({
    ...plan,
    side_effects: {
      ...plan.side_effects,
      transition_to: transitionOn ? origTransition : null,
      comment: commentOn ? commentText : null,
    },
  });
  const errors = $derived(validatePlan(built));
  const valid = $derived(planValid(errors));

  function describe(t: TransitionTarget | null): string {
    if (!t) return 'next status';
    return 'category' in t ? t.category.replace('_', ' ') : t.name;
  }

  // ---- progress (driven by work.updated) ----------------------------------------------------
  const live = $derived<WorkItem | null>(
    startedItem
      ? (work.get(startedItem.id) ?? startedItem)
      : phase === 'edit'
        ? null
        : (work.all.find(
            (w) =>
              w.project_id === plan.project_id &&
              w.branch.startsWith(plan.branch) &&
              Date.parse(w.created_at) >= startedAt - 5000,
          ) ?? null),
  );
  const steps = $derived.by(() => {
    const byName = new Map((live?.steps ?? []).map((s) => [s.step, s]));
    const names = [
      ...START_STEPS,
      ...(live?.steps ?? []).map((s) => s.step).filter((s) => !START_STEPS.includes(s)),
    ];
    return names.map((name) => ({
      name,
      status: byName.get(name)?.status ?? 'pending',
      detail: byName.get(name)?.detail ?? null,
    }));
  });
  const failedStep = $derived(
    live?.state.kind === 'failed'
      ? live.state
      : steps.some((s) => s.status === 'failed')
        ? { step: steps.find((s) => s.status === 'failed')?.name ?? '', message: failure?.message ?? '' }
        : null,
  );

  $effect(() => {
    // Backend reported the saga as failed through work.updated while work_start is still pending/rejected.
    if (phase === 'running' && live?.state.kind === 'failed') phase = 'failed';
  });

  const ticketUrl = $derived.by(() => {
    if (plan.source.kind !== 'ticket') return null;
    const ref = plan.source.ticket;
    return tickets.details[`${ref.account}:${ref.key}`]?.data?.ticket.url ?? null;
  });

  async function start(): Promise<void> {
    touched = true;
    if (!valid || phase === 'running') return;
    const toSend = $state.snapshot(built) as StartWorkPlan;
    if (resuming && seed.existing) {
      phase = 'running';
      try {
        const item = await workResume({ id: seed.existing });
        work.upsert(item);
        toasts.info(`Resumed ${item.branch}`);
        onclose();
      } catch (err) {
        failure = toIpcError('work_resume', err).toKeltaError();
        phase = 'failed';
      }
      return;
    }
    phase = 'running';
    failure = null;
    startedAt = Date.now();
    try {
      const item = await workStart({ plan: toSend });
      startedItem = item;
      work.upsert(item);
      if (item.state.kind === 'failed') {
        phase = 'failed';
        return;
      }
      toasts.info(`Started ${item.branch}`);
      onclose();
    } catch (err) {
      failure = toIpcError('work_start', err).toKeltaError();
      phase = 'failed';
    }
  }

  async function retry(step: string): Promise<void> {
    const item = live;
    if (!item) return;
    retrying = step;
    try {
      const next = await workRetryStep({ id: item.id, step });
      startedItem = next;
      work.upsert(next);
      failure = null;
      if (next.state.kind === 'failed') {
        phase = 'failed';
      } else {
        toasts.info(`Started ${next.branch}`);
        onclose();
      }
    } catch (err) {
      failure = toIpcError('work_retry_step', err).toKeltaError();
      phase = 'failed';
    } finally {
      retrying = null;
    }
  }

  function browse(): void {
    if (ticketUrl) openExternal({ url: ticketUrl }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && phase === 'edit') {
      e.preventDefault();
      void start();
    }
  }

  const efforts: { value: ClaudeEffort; label: string }[] = [
    { value: 'low', label: 'low' },
    { value: 'medium', label: 'medium' },
    { value: 'high', label: 'high' },
    { value: 'xhigh', label: 'xhigh' },
    { value: 'max', label: 'max' },
  ];
  const modes: { value: PermissionMode; label: string }[] = [
    { value: 'default', label: 'default' },
    { value: 'acceptEdits', label: 'accept edits' },
    { value: 'plan', label: 'plan' },
    { value: 'auto', label: 'auto' },
    { value: 'dontAsk', label: "don't ask" },
    { value: 'bypassPermissions', label: 'bypass permissions' },
  ];
  const choices: { value: BranchChoice; label: string }[] = [
    { value: 'reuse', label: 'Reuse the existing branch' },
    { value: 'suffix', label: 'Create a new branch with a suffix' },
  ];
</script>

<Sheet {title} width={560} {onclose}>
  {#if phase === 'edit'}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <form
      id="start-work-form"
      class="form"
      data-testid="start-work-form"
      onsubmit={(e) => {
        e.preventDefault();
        void start();
      }}
      {onkeydown}
    >
      {#if resuming}
        <p class="note" data-testid="resume-note">
          A work item already exists for {subject} on branch <code>{plan.branch}</code>. Resume it: its tab
          and sessions are focused or recreated.
        </p>
        <dl class="facts">
          <dt>Branch</dt>
          <dd><code>{plan.branch}</code></dd>
          <dt>Worktree</dt>
          <dd><code>{plan.worktree_path}</code></dd>
        </dl>
      {:else}
        {#if plan.repo_choices.length > 1}
          <Select
            label="Repository"
            bind:value={plan.repo_id}
            options={plan.repo_choices.map((r) => ({ value: r, label: r }))}
          />
        {/if}
        <div class="two">
          <TextInput label="Base branch" bind:value={plan.base} error={touched ? errors.base : null} />
          <TextInput
            label="Branch"
            bind:value={plan.branch}
            error={errors.branch ?? (touched ? errors.repo : null)}
            autofocus
          />
        </div>
        {#if plan.branch_exists}
          <div class="warn" role="note">
            <Icon name="triangle-alert" size={14} />
            <span>
              Branch <code>{plan.branch}</code> already exists{plan.branch_exists.has_worktree
                ? ' and has a worktree'
                : ''}.
            </span>
          </div>
          <Select
            label="When the branch exists"
            value={plan.branch_exists.choice}
            options={choices}
            onchange={(v) => {
              if (plan.branch_exists) plan.branch_exists = { ...plan.branch_exists, choice: v };
            }}
          />
        {/if}
        <TextInput label="Worktree path" value={plan.worktree_path} readonly />

        <fieldset>
          <legend>Claude ({plan.claude.profile})</legend>
          <div class="two">
            <TextInput label="Model" bind:value={plan.claude.model} />
            <Select label="Effort" bind:value={plan.claude.effort} options={efforts} />
          </div>
          <Select label="Permission mode" bind:value={plan.claude.permission_mode} options={modes} />
          <TextInput
            label="Prompt"
            bind:value={plan.claude.prompt}
            multiline
            rows={6}
            error={errors.prompt}
          />
        </fieldset>

        <fieldset>
          <legend>Side effects</legend>
          {#if isTicket}
            <Toggle label="Assign the ticket to me" bind:checked={plan.side_effects.assign_me} />
            {#if origTransition}
              <Toggle label={`Move the ticket to ${describe(origTransition)}`} bind:checked={transitionOn} />
            {/if}
            <Toggle label="Comment on the ticket" bind:checked={commentOn} />
            {#if commentOn}
              <TextInput label="Comment" bind:value={commentText} multiline rows={2} error={errors.comment} />
            {/if}
          {/if}
          <Toggle label="Run setup commands" bind:checked={plan.side_effects.run_setup} />
        </fieldset>
      {/if}
    </form>
  {:else}
    <div class="progress" data-testid="start-progress">
      {#if phase === 'running'}
        <p class="note"><Spinner size={14} /> {resuming ? 'Resuming' : 'Starting'} {subject}…</p>
      {/if}
      {#if !resuming}
        <ol class="steps" aria-label="Progress">
          {#each steps as s (s.name)}
            <li class={s.status} data-step={s.name} data-status={s.status}>
              {#if s.status === 'done'}
                <Icon name="circle-check" size={14} />
              {:else if s.status === 'failed'}
                <Icon name="circle-x" size={14} />
              {:else if s.status === 'running'}
                <Spinner size={14} />
              {:else if s.status === 'skipped'}
                <Icon name="minus" size={14} />
              {:else}
                <Icon name="circle" size={14} />
              {/if}
              <span>{stepLabel(s.name)}</span>
              {#if s.detail}<small>{s.detail}</small>{/if}
              {#if s.status === 'failed' || failedStep?.step === s.name}
                <span class="step-actions">
                  <Button size="sm" loading={retrying === s.name} onclick={() => void retry(s.name)}
                    >Retry</Button
                  >
                </span>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}
      {#if phase === 'failed'}
        <div class="fail" role="alert" data-testid="start-failed">
          <Icon name="circle-alert" size={14} />
          <span>{failedStep?.message || failure?.message || 'Starting the work item failed'}</span>
        </div>
        {#if resuming}
          <Button onclick={() => void start()}>Retry</Button>
        {/if}
      {/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if phase === 'failed'}
      {#if ticketUrl}<Button variant="ghost" icon="external-link" onclick={browse}>Open in browser</Button
        >{/if}
      <Button variant="ghost" onclick={() => (phase = 'edit')} disabled={live !== null}>Back</Button>
      <Button variant="ghost" onclick={onclose}>Close</Button>
    {:else if phase === 'edit'}
      <Button variant="ghost" onclick={onclose}>Cancel</Button>
      <Button variant="primary" type="submit" form="start-work-form" disabled={!valid && touched}>
        {resuming ? 'Resume' : 'Start'}
      </Button>
    {/if}
  {/snippet}
</Sheet>

<style>
  .form,
  .progress,
  fieldset {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  fieldset {
    margin: 0;
    padding: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  legend {
    padding: 0 var(--k-space-1);
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--k-space-3);
  }

  .note {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: 0;
    color: var(--k-fg-muted);
  }

  .warn,
  .fail {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    padding: var(--k-space-2) var(--k-space-3);
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    color: var(--k-warn);
  }

  .fail {
    color: var(--k-danger);
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--k-space-1) var(--k-space-3);
    margin: 0;
  }

  .facts dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .steps .pending {
    color: var(--k-fg-subtle);
  }

  .steps .done :global(.k-icon) {
    color: var(--k-ok);
  }

  .steps .failed {
    color: var(--k-danger);
  }

  .step-actions {
    margin-left: auto;
    display: inline-flex;
    gap: var(--k-space-1);
  }

  small {
    color: var(--k-fg-subtle);
  }
</style>
