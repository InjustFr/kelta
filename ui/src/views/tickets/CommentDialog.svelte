<script lang="ts">
  import type { TicketRef } from '$lib/gen';
  import { trackerComment } from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import { Button, Dialog, TextInput } from '$lib/ui';

  interface Props {
    ticket: TicketRef;
    onclose: () => void;
    ondone?: () => void;
  }

  let { ticket, onclose, ondone }: Props = $props();

  let text = $state('');
  let busy = $state(false);

  async function submit(): Promise<void> {
    if (text.trim() === '' || busy) return;
    busy = true;
    try {
      await trackerComment({ ticket, markdown: text });
      toasts.info(`Comment added to ${ticket.key}`);
      ondone?.();
      onclose();
    } catch (err) {
      toasts.error(err, `Commenting on ${ticket.key}`);
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void submit();
    }
  }
</script>

<Dialog title={`Comment on ${ticket.key}`} {onclose}>
  <TextInput bind:value={text} label="Comment (Markdown)" multiline rows={6} {onkeydown} />
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button variant="primary" loading={busy} disabled={text.trim() === ''} onclick={() => void submit()}>
      Comment
    </Button>
  {/snippet}
</Dialog>
