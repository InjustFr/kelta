<script lang="ts">
  import { PALETTE } from './draft';

  // Ten default chips plus a custom hex field; the value is stored in ProjectInfo.color as before.
  interface Props {
    value: string | null;
    onchange: (color: string | null) => void;
  }

  let { value, onchange }: Props = $props();

  const HEX = /^#[0-9a-f]{6}$/i;
  const custom = $derived(value && !PALETTE.some((c) => c.hex === value) ? value : '');
</script>

<div class="colors" role="group" aria-label="Colour">
  {#each PALETTE as c (c.name)}
    <button
      type="button"
      class="swatch"
      class:on={value === c.hex}
      style:background="var(--k-swatch-{c.name})"
      aria-label={c.name}
      title={c.name}
      aria-pressed={value === c.hex}
      onclick={() => onchange(value === c.hex ? null : c.hex)}
    ></button>
  {/each}
  <input
    class="hex k-mono"
    aria-label="Custom colour (hex)"
    placeholder="#rrggbb"
    maxlength="7"
    value={custom}
    onchange={(e) => {
      const v = e.currentTarget.value.trim();
      if (!v) onchange(null);
      else if (HEX.test(v)) onchange(v.toLowerCase());
      else e.currentTarget.value = custom;
    }}
  />
</div>

<style>
  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-2);
    align-items: center;
  }

  .swatch {
    width: 18px;
    height: 18px;
    padding: 0;
    border-radius: var(--k-radius-sm);
    border: none;
    cursor: pointer;
  }

  .swatch.on {
    box-shadow:
      0 0 0 2px var(--k-bg-float),
      0 0 0 3px var(--k-fg);
  }

  .hex {
    width: 80px;
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
    font-size: var(--k-font-size-sm);
  }
</style>
