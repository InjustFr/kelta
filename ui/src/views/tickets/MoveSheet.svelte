<script lang="ts">
  import type { SheetProps } from '$app/registry';
  import type { ProjectId, Ticket } from '$lib/gen';

  import StatusPicker from './StatusPicker.svelte';

  // Sheet `tickets.move` (the palette's "Move KEY to…"): the status picker without a pane.
  let {
    ticket,
    project_id = null,
    onclose,
  }: SheetProps & { ticket: Ticket; project_id?: ProjectId | null } = $props();
</script>

<StatusPicker tickets={[ticket]} projectId={project_id} {onclose} />
