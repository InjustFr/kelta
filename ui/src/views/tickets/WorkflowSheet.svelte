<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SheetProps } from '$app/registry';
  import type { TicketRef, WorkflowInfo } from '$lib/gen';

  import { rank } from '../../shell/palette/fuzzy';
  import Picker from '../../shell/palette/Picker.svelte';
  import { runWorkflow } from './workflow.svelte';

  // Sheet `workflow_run` (#233): the workflows `pickWorkflow` just listed; Enter runs one on the ticket.
  let { ticket, workflows, onclose }: SheetProps & { ticket: TicketRef; workflows: WorkflowInfo[] } =
    $props();

  let query = $state('');

  const items = $derived(
    rank(
      workflows.map((w) => ({
        id: w.name,
        label: w.name,
        detail: [w.description, w.source].filter(Boolean).join(' · '),
        icon: 'play',
      })),
      query,
      (i) => `${i.label} ${i.detail}`,
    ),
  );

  function pick(name: string): void {
    onclose();
    void runWorkflow(ticket, name);
  }
</script>

<Picker
  title={t('Run workflow on {key}', { key: ticket.key })}
  placeholder={t('Workflow name…')}
  {items}
  {query}
  onquery={(q) => (query = q)}
  onpick={pick}
  {onclose}
  testid="workflow-picker"
  empty={t('No matching workflow')}
/>
