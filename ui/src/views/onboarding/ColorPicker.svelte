<script lang="ts">
  import { parseProjectColor, resolveProjectColor, slotLabel, SLOTS, type Slot } from '$lib/theme';

  // The active theme's 12 slot colours, stored as `theme:<slot>` so they follow the theme, plus a
  // custom hex kept as typed (DESIGN §2.1).
  interface Props {
    value: string | null;
    onchange: (color: string | null) => void;
  }

  let { value, onchange }: Props = $props();

  const HEX = /^#[0-9a-f]{6}$/i;
  const slots = Object.keys(SLOTS) as Slot[];
  const parsed = $derived(parseProjectColor(value));
  const slot = $derived(parsed && 'slot' in parsed ? parsed.slot : null);
  const custom = $derived(parsed && 'hex' in parsed ? parsed.hex : '');
</script>

<div class="colors" role="group" aria-label="Colour">
  {#each slots as s (s)}
    <button
      type="button"
      class="swatch"
      class:on={slot === s}
      style:background={resolveProjectColor(`theme:${s}`)}
      aria-label={slotLabel(s)}
      title={slotLabel(s)}
      aria-pressed={slot === s}
      data-slot={s}
      onclick={() => onchange(slot === s ? null : `theme:${s}`)}
    ></button>
  {/each}
  {#if custom}
    <span class="custom" title="Custom {custom}" data-testid="custom-color">
      <span class="swatch on" style:background={custom}></span>Custom
    </span>
  {/if}
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

  .custom {
    display: inline-flex;
    gap: var(--k-space-2);
    align-items: center;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .custom .swatch {
    cursor: default;
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
