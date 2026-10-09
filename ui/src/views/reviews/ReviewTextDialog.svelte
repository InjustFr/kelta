<script lang="ts">
  import { Button, Dialog, TextInput } from '$lib/ui';

  interface Props {
    title: string;
    label: string;
    submitLabel: string;
    /** Submit is disabled while the text is empty. */
    required?: boolean;
    danger?: boolean;
    onsubmit: (text: string) => Promise<void>;
    onclose: () => void;
  }

  let { title, label, submitLabel, required = false, danger = false, onsubmit, onclose }: Props = $props();

  let text = $state('');
  let busy = $state(false);

  async function submit(): Promise<void> {
    if (busy || (required && text.trim() === '')) return;
    busy = true;
    try {
      await onsubmit(text);
      onclose();
    } catch {
      // The caller reports the error with a toast; keep the dialog open to retry.
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

<Dialog {title} {onclose}>
  <TextInput bind:value={text} {label} multiline rows={6} {onkeydown} />
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button
      variant={danger ? 'danger' : 'primary'}
      loading={busy}
      disabled={required && text.trim() === ''}
      onclick={() => void submit()}
    >
      {submitLabel}
    </Button>
  {/snippet}
</Dialog>
