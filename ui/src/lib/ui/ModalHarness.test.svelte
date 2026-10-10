<script lang="ts">
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';
  import Sheet from './Sheet.svelte';
  import TextInput from './TextInput.svelte';
  import Toggle from './Toggle.svelte';

  interface Props {
    kind: 'dialog' | 'sheet';
    onsubmit: (draft: boolean) => void;
    onclose: () => void;
  }

  let { kind, onsubmit, onclose }: Props = $props();
  const Modal = $derived(kind === 'dialog' ? Dialog : Sheet);
  let title = $state('');
  let draft = $state(false);
</script>

<button type="button">outside</button>
<Modal title="Ship" {onclose}>
  <TextInput label="Title" bind:value={title} />
  <Toggle label="Draft" bind:checked={draft} />
  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button variant="primary" onclick={() => onsubmit(draft)}>Ship</Button>
  {/snippet}
</Modal>
