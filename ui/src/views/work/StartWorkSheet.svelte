<script lang="ts">
  import { msg, t } from '$lib/i18n/index.svelte';
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type {
    BranchChoice,
    ClaudeEffort,
    KeltaError,
    PermissionMode,
    StartOption,
    StartWorkPlan,
    StatusCategory,
    TransitionTarget,
    WorkItem,
  } from '$lib/gen';
  import {
    openExternal,
    settingsSet,
    workPlan,
    workResume,
    workRetryStep,
    workStart,
  } from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, settings, tickets, toasts, work } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import { Button, Icon, Select, Sheet, Spinner, TextInput, Toggle } from '$lib/ui';

  import { activateProject } from '../../shell/nav';
  import { withCriteria } from '../tickets/refine.svelte';
  import {
    applyOptions,
    optionsText,
    planValid,
    START_STEPS,
    stepLabel,
    unapplyOptions,
    validatePlan,
  } from './common';
  import FinishFirst from './FinishFirst.svelte';
  import { finishFirst } from './finishFirst';

  interface Props extends SheetProps {
    plan: StartWorkPlan;
  }

  let { onclose, plan: initial }: Props = $props();

  const seed = untrack(() => structuredClone($state.snapshot(initial)) as StartWorkPlan);
  let plan = $state<StartWorkPlan>(seed);
  let origTransition = $state<TransitionTarget | null>(seed.side_effects.transition_to);
  let transitionOn = $state(seed.side_effects.transition_to !== null);
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
  // Finish before starting (#354): a step before the plan, never for a resume.
  const unfinished = $derived(finishFirst(seed));
  let asking = $state(untrack(() => unfinished.length > 0));

  async function stopAsking(): Promise<void> {
    try {
      settings.accept(null, await settingsSet({ layer: 'global', path: 'work.finish_first', value: false }));
      asking = false;
    } catch (err) {
      toasts.error(err, t("Don't ask again"));
    }
  }
  const projectName = (id: string): string => projects.byId(id)?.name ?? id;
  // The guess hint names the ticket's source, else its account.
  const sourceName = (p: StartWorkPlan, account: string): string =>
    settings.value()?.sources.find((s) => s.id === p.ticket_source)?.label || account;
  let switching = $state(false);

  /** Re-plans in `id`: branch, worktree, Claude and side effects follow its settings (edits are dropped). */
  async function switchProject(id: string): Promise<void> {
    switching = true;
    try {
      const next = await workPlan({ project_id: id, source: plan.source, source_id: plan.ticket_source });
      if (next.source.kind === 'ticket') withCriteria(next, next.source.ticket);
      plan = { ...next, project_choices: plan.project_choices, project_guessed: plan.project_guessed };
      origTransition = next.side_effects.transition_to;
      transitionOn = origTransition !== null;
      commentOn = next.side_effects.comment !== null;
      commentText = next.side_effects.comment ?? '';
    } catch (err) {
      toasts.error(err, t('Switch project'));
    } finally {
      switching = false;
    }
  }
  // ---- start options (#357): the prompt keeps its `{options}` form in `plan.options.prompt` ----
  function editPrompt(v: string): void {
    plan.claude.prompt = v;
    const o = plan.options;
    if (o) o.prompt = unapplyOptions(v, optionsText(o.declared, o.picked));
  }
  function pick(opt: StartOption, value: string | null): void {
    const o = plan.options;
    if (!o) return;
    o.picked = [...o.picked.filter((v) => !opt.values.includes(v)), ...(value ? [value] : [])];
    plan.claude.prompt = applyOptions(o.prompt, o.declared, o.picked);
  }

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
      ? t('Resume {subject}', { subject })
      : plan.source.kind === 'review'
        ? t('Review locally {subject}', { subject })
        : plan.source.kind === 'branch'
          ? t('New branch workspace')
          : t('Start work on {subject}', { subject }),
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

  const CATEGORY: Record<StatusCategory, string> = {
    todo: msg('to do'),
    in_progress: msg('in progress'),
    in_review: msg('in review'),
    done: msg('done'),
    unknown: msg('unknown'),
  };
  function describe(to: TransitionTarget | null): string {
    if (!to) return t('next status');
    return 'category' in to ? t(CATEGORY[to.category]) : to.name;
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
    // a re-plan in flight: starting now would use the old project and branch
    if (switching) return;
    touched = true;
    if (!valid || phase === 'running') return;
    const toSend = $state.snapshot(built) as StartWorkPlan;
    if (resuming && seed.existing) {
      phase = 'running';
      try {
        const item = await workResume({ id: seed.existing });
        work.upsert(item);
        toasts.info(t('Resumed {branch}', { branch: item.branch }), { view: null });
        void activateProject(item.project_id);
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
      toasts.info(
        item.state.kind === 'queued'
          ? t('Queued {branch}', { branch: item.branch })
          : t('Started {branch}', { branch: item.branch }),
        { view: null },
      );
      // The work tab is the project's active tab: bring it to the front (leaves Now).
      void activateProject(item.project_id);
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
        toasts.info(t('Started {branch}', { branch: next.branch }));
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
    if (ticketUrl) openExternal({ url: ticketUrl }).catch((err) => toasts.error(err, t('Open in browser')));
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && phase === 'edit') {
      e.preventDefault();
      void start();
    }
  }

  const efforts: { value: ClaudeEffort; label: string }[] = $derived([
    { value: 'low', label: t('low') },
    { value: 'medium', label: t('medium') },
    { value: 'high', label: t('high') },
    { value: 'xhigh', label: t('xhigh') },
    { value: 'max', label: t('max') },
  ]);
  // '' = no forced mode: the last one used, else Claude's own default.
  const modes: { value: PermissionMode | ''; label: string }[] = $derived([
    { value: '', label: t('last used') },
    { value: 'default', label: t('default') },
    { value: 'acceptEdits', label: t('accept edits') },
    { value: 'plan', label: t('plan') },
    { value: 'auto', label: t('auto') },
    { value: 'dontAsk', label: t("don't ask") },
    { value: 'bypassPermissions', label: t('bypass permissions') },
  ]);
  const choices: { value: BranchChoice; label: string }[] = $derived([
    { value: 'reuse', label: t('Reuse the existing branch') },
    { value: 'suffix', label: t('Create a new branch with a suffix') },
  ]);
</script>

<Sheet {title} width={560} {onclose}>
  {#if asking}
    <FinishFirst rows={unfinished} ongo={onclose} />
  {:else if phase === 'edit'}
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
          {t(
            'A work item already exists for {subject} in {project} on branch {branch}. Resume it: its tab and sessions are focused or recreated.',
            { subject, project: projectName(plan.project_id), branch: plan.branch },
          )}
        </p>
        <dl class="facts">
          <dt>{t('Branch')}</dt>
          <dd><code>{plan.branch}</code></dd>
          <dt>{t('Worktree')}</dt>
          <dd><code>{plan.worktree_path}</code></dd>
        </dl>
      {:else}
        {#if plan.project_choices.length > 1}
          <div
            class="project"
            style:--bar={resolveProjectColor(projects.byId(plan.project_id)?.color) ?? 'transparent'}
          >
            <Select
              label={t('Project')}
              value={plan.project_id}
              disabled={switching}
              options={plan.project_choices.map((id) => ({ value: id, label: projectName(id) }))}
              onchange={(id) => void switchProject(id)}
            />
            {#if plan.project_guessed && plan.source.kind === 'ticket'}
              <p class="hint" data-testid="project-guess">
                {t('No usual project for {source} tickets yet. Kelta will remember this one.', {
                  source: sourceName(plan, plan.source.ticket.account),
                })}
              </p>
            {/if}
          </div>
        {/if}
        {#if plan.repo_choices.length > 1}
          <Select
            label={t('Repository')}
            bind:value={plan.repo_id}
            options={plan.repo_choices.map((r) => ({ value: r, label: r }))}
          />
        {/if}
        <div class="two">
          <TextInput label={t('Base branch')} bind:value={plan.base} error={touched ? errors.base : null} />
          <TextInput
            label={t('Branch')}
            bind:value={plan.branch}
            error={errors.branch ?? (touched ? errors.repo : null)}
            autofocus={!seed.project_guessed}
          />
        </div>
        {#if plan.branch_exists}
          <div class="warn" role="note">
            <Icon name="triangle-alert" size={14} />
            <span>
              {plan.branch_exists.has_worktree
                ? t('Branch {branch} already exists and has a worktree.', { branch: plan.branch })
                : t('Branch {branch} already exists.', { branch: plan.branch })}
            </span>
          </div>
          <Select
            label={t('When the branch exists')}
            value={plan.branch_exists.choice}
            options={choices}
            onchange={(v) => {
              if (plan.branch_exists) plan.branch_exists = { ...plan.branch_exists, choice: v };
            }}
          />
        {/if}
        <TextInput label={t('Worktree path')} value={plan.worktree_path} readonly />

        <fieldset>
          <legend>Claude ({plan.claude.profile})</legend>
          <div class="two">
            <TextInput label={t('Model')} bind:value={plan.claude.model} />
            <Select label={t('Effort')} bind:value={plan.claude.effort} options={efforts} />
          </div>
          <Select
            label={t('Permission mode')}
            bind:value={
              () => plan.claude.permission_mode ?? '', (v) => (plan.claude.permission_mode = v || null)
            }
            options={modes}
          />
          <TextInput
            label={t('Prompt')}
            bind:value={() => plan.claude.prompt, editPrompt}
            multiline
            rows={6}
            error={errors.prompt}
          />
          {#if plan.options}
            {@const picked = plan.options.picked}
            <div class="options" data-testid="start-options">
              {#each plan.options.declared as o, i (i)}
                {#if o.values.length === 1}
                  <Toggle
                    label={o.label}
                    checked={picked.includes(o.values[0]!)}
                    onchange={(on) => pick(o, on ? o.values[0]! : null)}
                  />
                {:else if o.values.length > 1}
                  <Select
                    label={o.label}
                    value={o.values.find((v) => picked.includes(v)) ?? ''}
                    options={[
                      { value: '', label: t('None') },
                      ...o.values.map((v) => ({ value: v, label: v })),
                    ]}
                    onchange={(v) => pick(o, v || null)}
                  />
                {/if}
              {/each}
            </div>
          {/if}
        </fieldset>

        <fieldset>
          <legend>{t('Side effects')}</legend>
          {#if isTicket}
            <Toggle label={t('Assign the ticket to me')} bind:checked={plan.side_effects.assign_me} />
            {#if origTransition}
              <Toggle
                label={t('Move the ticket to {status}', { status: describe(origTransition) })}
                bind:checked={transitionOn}
              />
            {/if}
            <Toggle label={t('Comment on the ticket')} bind:checked={commentOn} />
            {#if commentOn}
              <TextInput
                label={t('Comment')}
                bind:value={commentText}
                multiline
                rows={2}
                error={errors.comment}
              />
            {/if}
          {/if}
          <Toggle label={t('Run setup commands')} bind:checked={plan.side_effects.run_setup} />
        </fieldset>
      {/if}
    </form>
  {:else}
    <div class="progress" data-testid="start-progress">
      {#if phase === 'running'}
        <p class="note">
          <Spinner size={14} />
          {resuming ? t('Resuming {subject}…', { subject }) : t('Starting {subject}…', { subject })}
        </p>
      {/if}
      {#if !resuming}
        <ol class="steps" aria-label={t('Progress')}>
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
                    >{t('Retry')}</Button
                  >
                  <Button
                    size="sm"
                    variant="ghost"
                    loading={retrying === `skip:${s.name}`}
                    onclick={() => void retry(`skip:${s.name}`)}>{t('Skip')}</Button
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
          <span>{failedStep?.message || failure?.message || t('Starting the work item failed')}</span>
        </div>
        {#if resuming}
          <Button onclick={() => void start()}>{t('Retry')}</Button>
        {/if}
      {/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if asking}
      <Button variant="ghost" onclick={onclose}>{t('Cancel')}</Button>
      <Button variant="ghost" onclick={() => void stopAsking()}>{t("Don't ask again")}</Button>
      <Button variant="primary" onclick={() => (asking = false)}>{t('Start anyway')}</Button>
    {:else if phase === 'failed'}
      {#if ticketUrl}<Button variant="ghost" icon="external-link" onclick={browse}
          >{t('Open in browser')}</Button
        >{/if}
      <Button variant="ghost" onclick={() => (phase = 'edit')} disabled={live !== null}>{t('Back')}</Button>
      <Button variant="ghost" onclick={onclose}>{t('Close')}</Button>
    {:else if phase === 'edit'}
      <Button variant="ghost" onclick={onclose}>{t('Cancel')}</Button>
      <Button
        variant="primary"
        type="submit"
        form="start-work-form"
        disabled={switching || (!valid && touched)}
      >
        {resuming ? t('Resume') : t('Start')}
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

  .options {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--k-space-3);
  }

  .project {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-1);
    padding-left: var(--k-space-2);
    border-left: 3px solid var(--bar);
  }

  .hint {
    margin: 0;
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
