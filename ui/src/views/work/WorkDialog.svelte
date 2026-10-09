<script lang="ts">
  // One sentence, one or two actions (DESIGN §6.13): rebase refusals, fetch failure, force push.
  // Opened as a sheet entry so any caller (palette, toast, Now) can show it.
  import type { SheetProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import { Button, Dialog } from '$lib/ui';

  import type { DialogAction, WorkDialogProps } from './fixloop.svelte';

  type Props = SheetProps & WorkDialogProps;

  let { onclose, title, text, tone = 'default', actions: buttons }: Props = $props();

  function run(a: DialogAction): void {
    onclose();
    if (a.command) void dispatch(a.command, a.args);
  }

  function onkeydown(e: KeyboardEvent): void {
    const primary = buttons[buttons.length - 1];
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && primary) {
      e.preventDefault();
      run(primary);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div {onkeydown}>
  <Dialog {title} {tone} width={480} {onclose}>
    <p class="text" data-testid="work-dialog-text">{text}</p>
    {#snippet actions()}
      {#each buttons as a (a.label)}
        <Button variant={a.variant ?? 'secondary'} onclick={() => run(a)}>{a.label}</Button>
      {/each}
    {/snippet}
  </Dialog>
</div>

<style>
  .text {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
