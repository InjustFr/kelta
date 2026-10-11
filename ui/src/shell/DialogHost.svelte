<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { Button, Dialog, TextInput, Toggle } from '$lib/ui';

  import { confirms, prompts } from './confirm.svelte';

  let text = $derived(prompts.current?.value ?? '');
</script>

{#if confirms.current}
  {@const req = confirms.current}
  <Dialog title={req.title} tone={req.tone ?? 'default'} onclose={() => confirms.answer(null)}>
    <div data-testid="confirm-dialog">
      <p class="body">{req.body}</p>
      {#if req.details?.length}
        <ul>
          {#each req.details as line, i (i)}<li>{line}</li>{/each}
        </ul>
      {/if}
      {#if req.checkbox}
        <div class="check" data-testid="confirm-checkbox">
          <Toggle label={req.checkbox} bind:checked={confirms.checked} />
        </div>
      {/if}
    </div>
    {#snippet actions()}
      <Button onclick={() => confirms.answer(null)}>{t('Cancel')}</Button>
      {#each req.actions as a (a.id)}
        <Button
          variant={a.variant ?? 'secondary'}
          onclick={() => confirms.answer(a.id)}
          data-testid="confirm-{a.id}">{a.label}</Button
        >
      {/each}
    {/snippet}
  </Dialog>
{/if}

{#if prompts.current}
  {@const req = prompts.current}
  <Dialog title={req.title} onclose={() => prompts.answer(null)}>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        prompts.answer(text);
      }}
    >
      <TextInput label={req.label} type={req.type ?? 'text'} autocomplete="off" bind:value={text} />
    </form>
    {#snippet actions()}
      <Button onclick={() => prompts.answer(null)}>{t('Cancel')}</Button>
      <Button variant="primary" onclick={() => prompts.answer(text)}>{req.confirmLabel ?? t('OK')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .body {
    margin: 0 0 var(--k-space-3);
    color: var(--k-fg-muted);
  }

  .check {
    margin-top: var(--k-space-3);
  }

  ul {
    margin: 0;
    padding-left: var(--k-space-5);
    color: var(--k-fg);
  }
</style>
