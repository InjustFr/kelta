<script lang="ts">
  import { untrack } from 'svelte';

  import type { SheetProps } from '$app/registry';
  import type { ClaudeEffort } from '$lib/gen';
  import { workPlan, workStart } from '$lib/ipc/commands';
  import { settings, toasts, work } from '$lib/stores';
  import { Button, Select, Sheet, TextInput } from '$lib/ui';

  import { railProjects } from '../../shell/nav';
  import { withCriteria } from '../tickets/refine.svelte';
  import { batch, claudeSlots, startsNow } from './batch.svelte';

  let { onclose }: SheetProps = $props();

  const marked = untrack(() => batch.list);
  const s = untrack(() => settings.value());
  const templates = (s?.session_templates ?? []).filter((t) => t.enabled);
  const profile = s?.claude.profiles['default'];
  let template = $state(
    templates.find((t) => t.id === 'claude+editor')?.id ?? templates[0]?.id ?? 'claude+editor',
  );
  let model = $state(profile?.model ?? 'opus');
  let effort = $state<ClaudeEffort>(profile?.effort ?? 'high');
  // '' = each ticket in its usual project (SPEC §3.1); a guessed one is skipped.
  let forced = $state('');
  const projectOptions = [
    { value: '', label: 'Usual project for each ticket' },
    ...railProjects()
      // Home is never a work_plan choice: forcing it would fall back to the usual project
      .filter((p) => !p.builtin && p.repos.length > 0)
      .map((p) => ({ value: p.id, label: p.name })),
  ];

  const slots = $derived(claudeSlots());
  const now = $derived(startsNow(marked.length, slots.live, slots.max));
  const title = $derived(
    `Start ${marked.length} ticket${marked.length === 1 ? '' : 's'}: ${now} now, ${marked.length - now} queued`,
  );

  const efforts: { value: ClaudeEffort; label: string }[] = (
    ['low', 'medium', 'high', 'xhigh', 'max'] as const
  ).map((v) => ({ value: v, label: v }));

  // In order: the queue keeps the order the tickets were marked in.
  async function startAll(): Promise<void> {
    onclose();
    batch.clear();
    let started = 0;
    let queued = 0;
    for (const m of marked) {
      try {
        const plan = await workPlan({
          project_id: forced || null,
          // a plain copy: the marks are $state proxies, which the mock IPC cannot clone
          source: { kind: 'ticket', ticket: { ...m.ref } },
        });
        if (!forced && plan.project_guessed) {
          toasts.warn(`${m.ref.key} has no usual project: start it with s`);
          continue;
        }
        if (plan.existing) {
          toasts.info(`${m.ref.key} already has a work item`);
          continue;
        }
        withCriteria(plan, m.ref);
        plan.template_id = template;
        plan.claude = { ...plan.claude, model, effort };
        const item = work.upsert(await workStart({ plan }));
        if (item.state.kind === 'queued') queued++;
        else started++;
      } catch (err) {
        toasts.error(err, `Start ${m.ref.key}`);
      }
    }
    toasts.info(`Started ${started}, queued ${queued}`);
  }
</script>

<Sheet {title} width={480} {onclose}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    class="form"
    data-testid="batch-start"
    onsubmit={(e) => {
      e.preventDefault();
      void startAll();
    }}
    onkeydown={(e) => {
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        void startAll();
      }
    }}
  >
    <p class="keys">{marked.map((m) => m.ref.key).join(', ')}</p>
    <Select label="Project" bind:value={forced} options={projectOptions} />
    <Select
      label="Template"
      bind:value={template}
      options={templates.map((t) => ({ value: t.id, label: t.label }))}
    />
    <div class="two">
      <TextInput label="Model" bind:value={model} />
      <Select label="Effort" bind:value={effort} options={efforts} />
    </div>
    <p class="hint">
      Claude {slots.live}/{slots.max || '∞'} live. Queued tickets get their worktree now and start, oldest first,
      when a Claude exits.
    </p>
  </form>
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button variant="primary" chord="mod+enter" onclick={() => void startAll()}>Start {marked.length}</Button>
  {/snippet}
</Sheet>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--k-space-3);
  }

  .keys,
  .hint {
    margin: 0;
    color: var(--k-fg-muted);
  }
</style>
