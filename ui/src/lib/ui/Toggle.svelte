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
    width: 30px;
    height: 18px;
    border-radius: 9px;
    background: var(--k-border-strong);
    transition: background var(--k-duration);
    flex: none;
  }

  .thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    transition: transform var(--k-duration);
  }

  input:checked + .track {
    background: var(--k-accent);
  }

  input:checked + .track .thumb {
    transform: translateX(12px);
  }

  input:focus-visible + .track {
    outline: 2px solid var(--k-focus);
    outline-offset: 2px;
  }
</style>
