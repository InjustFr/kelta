<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';

  interface Props extends Omit<HTMLInputAttributes, 'value'> {
    value?: string;
    label?: string;
    /** Validation message shown under the field. */
    error?: string | null;
    hint?: string;
    multiline?: boolean;
    rows?: number;
  }

  let {
    value = $bindable(''),
    label,
    error = null,
    hint,
    multiline = false,
    rows = 4,
    id,
    ...rest
  }: Props = $props();

  const uid = $props.id();
  const inputId = $derived(id ?? `k-input-${uid}`);
</script>

<div class="k-field" class:invalid={!!error}>
  {#if label}<label for={inputId}>{label}</label>{/if}
  {#if multiline}
    <textarea
      id={inputId}
      bind:value
      {rows}
      aria-invalid={!!error || undefined}
      placeholder={rest.placeholder}
      disabled={rest.disabled}
      readonly={rest.readonly}
      spellcheck={rest.spellcheck}
      onkeydown={rest.onkeydown as never}></textarea>
  {:else}
    <input {...rest} id={inputId} bind:value aria-invalid={!!error || undefined} />
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{:else if hint}<p class="hint">{hint}</p>{/if}
</div>

<style>
  .k-field {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    min-width: 0;
  }

  label {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  input,
  textarea {
    min-height: var(--k-control-height);
    padding: var(--k-space-2) var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    border-color: var(--k-focus);
  }

  .invalid input,
  .invalid textarea {
    border-color: var(--k-danger);
  }

  .error,
  .hint {
    margin: 0;
    font-size: var(--k-font-size-xs);
  }

  .error {
    color: var(--k-danger);
  }

  .hint {
    color: var(--k-fg-subtle);
  }
</style>
