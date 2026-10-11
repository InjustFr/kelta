<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SheetProps } from '$app/registry';
  import type { ProjectId, ReviewRef, TicketRef, WorkflowInfo } from '$lib/gen';
  import { projects } from '$lib/stores';

  import { rank } from '../../shell/palette/fuzzy';
  import Picker from '../../shell/palette/Picker.svelte';
  import { runWorkflow, runWorkflowOn } from './workflow.svelte';

  // Sheet `workflow_run` (#233): the workflows `pickWorkflow` just listed; Enter runs one on the ticket,
  // or on the PR `pickPrWorkflow` passed (#355).
  let {
    ticket,
    review,
    project_id,
    workflows,
    onclose,
  }: SheetProps & {
    ticket?: TicketRef;
    review?: ReviewRef;
    project_id?: ProjectId | null;
    workflows: WorkflowInfo[];
  } = $props();

  let query = $state('');

  const items = $derived(
    rank(
      workflows.map((w) => ({
        id: w.name,
        label: w.label,
        detail: [w.description, w.source].filter(Boolean).join(' · '),
        icon: 'play',
      })),
      query,
      (i) => `${i.label} ${i.detail}`,
    ),
  );

  function pick(name: string): void {
    onclose();
    const w = workflows.find((x) => x.name === name);
    if (ticket) void runWorkflow(ticket, name);
    else if (review && w) {
      const project = project_id ?? projects.activeId;
      if (project) void runWorkflowOn(project, w, review);
    }
  }
</script>

<Picker
  title={t('Run workflow on {key}', {
    key: ticket ? ticket.key : `${review?.repo}#${review?.number}`,
  })}
  placeholder={t('Workflow name…')}
  {items}
  {query}
  onquery={(q) => (query = q)}
  onpick={pick}
  {onclose}
  testid="workflow-picker"
  empty={t('No matching workflow')}
/>
