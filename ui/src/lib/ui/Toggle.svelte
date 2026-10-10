<script lang="ts">
  interface Props {
    checked?: boolean;
    label: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  }

  let { checked = $bindable(false), label, disabled = false, onchange }: Props = $props();
</script>

<label class="k-toggle" class:disabled>
  <input type="checkbox" role="switch" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span class="track" aria-hidden="true"><span class="thumb"></span></span>
  <span class="text">{label}</span>
</label>

<style>
  .k-toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-3);
    cursor: pointer;
  }

  .k-toggle.disabled {
    opacity: 0.55;
    cursor: default;
  }

  input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }

  .track {
    position: relative;
    width: 28px;
    height: 16px;
    border-radius: 8px;
    background: var(--k-bezel);
    box-shadow: inset 0 0 0 1px var(--k-border-strong);
    transition: background var(--k-duration) ease-out;
    flex: none;
  }

  .thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--k-well);
    box-shadow: 0 0 0 1px var(--k-border-strong);
    transition: transform var(--k-duration) ease-out;
  }

  input:checked + .track {
    background: var(--k-accent);
    box-shadow: none;
  }

  input:checked + .track .thumb {
    transform: translateX(12px);
    box-shadow: none;
  }

  /* Notch on the on side, so state is not shown by colour alone. */
  input:checked + .track .thumb::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 5.5px;
    width: 1px;
    height: 6px;
    background: var(--k-accent);
  }

  input:focus-visible + .track {
    outline: 2px solid var(--k-focus);
    outline-offset: 2px;
  }
</style>
