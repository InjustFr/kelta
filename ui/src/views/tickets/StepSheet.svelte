<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SheetProps } from '$app/registry';
  import type { StepDef, TicketRef } from '$lib/gen';

  import { rank } from '../../shell/palette/fuzzy';
  import Picker from '../../shell/palette/Picker.svelte';
  import { moveStep } from './step';

  // Sheet `tickets.step` (#463): the pipeline steps `pickStep` just listed; Enter moves the ticket there.
  let { ticket, steps, onclose }: SheetProps & { ticket: TicketRef; steps: StepDef[] } = $props();

  let query = $state('');

  const items = $derived(
    rank(
      steps.map((s) => ({ id: s.id, label: s.label || s.id, detail: s.id, icon: 'arrow-right' })),
      query,
      (i) => `${i.label} ${i.detail}`,
    ),
  );

  function pick(id: string): void {
    onclose();
    void moveStep(ticket, id);
  }
</script>

<Picker
  title={t('Move {key} to step', { key: ticket.key })}
  placeholder={t('Step…')}
  {items}
  {query}
  onquery={(q) => (query = q)}
  onpick={pick}
  {onclose}
  testid="step-picker"
  empty={t('No matching step')}
/>
