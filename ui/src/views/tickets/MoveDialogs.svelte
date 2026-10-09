<script lang="ts">
  import { openExternal } from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import { Button, Dialog, Select, TextInput } from '$lib/ui';

  import type { MoveController } from './move.svelte';

  let { move }: { move: MoveController } = $props();

  let values = $state<Record<string, string>>({});
  let touched = $state(false);

  const dialog = $derived(move.dialog);
  const fields = $derived(dialog?.kind === 'fields' ? dialog.fields : []);
  const missing = $derived(fields.filter((f) => f.required && (values[f.id] ?? '').trim() === ''));

  $effect(() => {
    // Reset the form whenever a new fields dialog opens.
    if (dialog?.kind === 'fields') {
      values = Object.fromEntries(dialog.fields.map((f) => [f.id, f.options[0]?.value ?? '']));
      touched = false;
    }
  });

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    touched = true;
    if (missing.length > 0) return;
    void move.submitFields(values);
  }

  function browser(url: string): void {
    openExternal({ url }).catch((err) => toasts.error(err, 'Open in browser'));
  }

  function listKeys(e: KeyboardEvent): void {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const buttons = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('button')];
    const at = buttons.indexOf(document.activeElement as HTMLElement);
    const next = buttons[(at + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length];
    if (next) {
      e.preventDefault();
      next.focus();
    }
  }
</script>

{#if dialog?.kind === 'pick'}
  <Dialog title={`Move ${dialog.ticket.ref.key} to ${dialog.target}`} onclose={() => move.cancel()}>
    <p class="hint">Several transitions lead there. Pick one.</p>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="choices" role="group" aria-label="Transitions" onkeydown={listKeys}>
      {#each dialog.candidates as t (t.id)}
        <Button variant="secondary" onclick={() => void move.choose(t)}>
          {t.name}{t.to.name !== t.name ? ` → ${t.to.name}` : ''}
        </Button>
      {/each}
    </div>
  </Dialog>
{:else if dialog?.kind === 'fields'}
  <Dialog title={`Move ${dialog.ticket.ref.key} to ${dialog.target}`} onclose={() => move.cancel()}>
    <form id="move-fields" class="form" onsubmit={submit}>
      <p class="hint">{dialog.message}</p>
      {#if fields.length === 0}
        <p class="hint">The tracker did not say which fields are missing.</p>
      {/if}
      {#each fields as f (f.id)}
        {#if f.options.length > 0}
          <Select
            label={f.name}
            options={f.options}
            value={values[f.id] ?? ''}
            onchange={(v) => (values = { ...values, [f.id]: v })}
          />
        {:else}
          <TextInput
            label={f.name}
            value={values[f.id] ?? ''}
            oninput={(e) => (values = { ...values, [f.id]: (e.currentTarget as HTMLInputElement).value })}
            error={touched && f.required && (values[f.id] ?? '').trim() === ''
              ? `${f.name} is required`
              : null}
          />
        {/if}
      {/each}
    </form>
    {#snippet actions()}
      <Button variant="ghost" onclick={() => browser(dialog.ticket.url)}>Open in browser</Button>
      <Button variant="ghost" onclick={() => move.cancel()}>Cancel</Button>
      <Button variant="primary" type="submit" form="move-fields" loading={move.busy}>Move</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .form,
  .choices {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .hint {
    margin: 0;
    color: var(--k-fg-muted);
  }
</style>
