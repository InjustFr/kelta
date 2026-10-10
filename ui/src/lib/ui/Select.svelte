<script lang="ts" generics="T extends string">
  interface Option {
    value: T;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    value?: T;
    options: readonly Option[];
    label?: string;
    disabled?: boolean;
    id?: string;
    onchange?: (value: T) => void;
  }

  let { value = $bindable(), options, label, disabled = false, id, onchange }: Props = $props();

  const uid = $props.id();
  const selectId = $derived(id ?? `k-select-${uid}`);
</script>

<div class="k-select">
  {#if label}<label for={selectId}>{label}</label>{/if}
  <select id={selectId} bind:value {disabled} onchange={() => value !== undefined && onchange?.(value)}>
    {#each options as opt (opt.value)}
      <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
    {/each}
  </select>
</div>

<style>
  .k-select {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  label {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  select {
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
  }
</style>
