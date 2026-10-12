<script lang="ts">
  import { initials } from '../work/common';
  import type { CountChip } from './filters';

  // Toggle chips that narrow a list to one value (tags #349, status and assignee #376); a second click
  // on the picked chip turns it off.
  interface Props {
    label: string;
    chips: readonly CountChip[];
    value: string | null;
    onchange: (id: string | null) => void;
  }

  let { label, chips, value, onchange }: Props = $props();
</script>

{#if chips.length > 0}
  <span class="chips" role="group" aria-label={label}>
    {#each chips as c (c.id)}
      <button
        type="button"
        class="chip"
        aria-pressed={value === c.id}
        data-chip={c.id}
        title={c.title}
        onclick={() => onchange(value === c.id ? null : c.id)}
        >{#if c.dot}<span class="dot" style:background={c.dot}></span>{/if}{#if c.avatar}<span
            class="k-avatar"
            aria-hidden="true">{initials(c.label)}</span
          >{/if}{c.label}<span class="k-num n">{c.n}</span></button
      >
    {/each}
  </span>
{/if}

<style>
  .chips {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--k-space-1);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--k-control-height-sm);
    padding: 0 var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: 999px;
    background: transparent;
    color: var(--k-fg-chrome);
    font-size: var(--k-font-size-xs);
    white-space: nowrap;
    cursor: pointer;
  }

  .chip:hover {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .chip[aria-pressed='true'] {
    border-color: var(--k-accent);
    background: var(--k-bg-selected);
    color: var(--k-fg);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .k-avatar {
    width: 16px;
    height: 16px;
    margin-left: -4px;
    font-size: 8px;
  }

  .n {
    color: var(--k-fg-subtle);
  }
</style>
