<script lang="ts">
  // Buttons for the active project's tools, in config order. Reads the cached tool list: no
  // polling, no timers. Embedded tools open or focus a pane, external ones launch (tools.open).
  import { dispatch } from '$lib/actions';
  import type { ProjectId } from '$lib/gen';
  import { tools } from '$lib/stores';
  import { Icon } from '$lib/ui';
  import { formatChord } from '$lib/ui/format';

  let { projectId }: { projectId: ProjectId } = $props();

  const list = $derived(tools.list(projectId));

  const initials = (label: string): string => label.trim().slice(0, 2);
  const hint = (label: string, chord: string | null, missing: boolean): string =>
    [label, chord ? `(${formatChord(chord).join('')})` : '', missing ? '- not installed' : '']
      .filter(Boolean)
      .join(' ');
</script>

{#if list.length > 0}
  <div class="strip" role="toolbar" aria-label="Tools" data-testid="tool-strip">
    {#each list as t (t.id)}
      <button
        type="button"
        class="tool"
        class:missing={t.installed === false}
        title={hint(t.label, t.keybinding, t.installed === false)}
        aria-label={t.label}
        data-tool-id={t.id}
        onclick={() => void dispatch('tools.open', { tool_id: t.id })}
      >
        {#if t.icon}<Icon name={t.icon} size={14} />{:else}{initials(t.label)}{/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .strip {
    position: sticky;
    right: 0;
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    padding-left: var(--k-space-3);
    background: var(--k-bg-elev);
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 22px;
    padding: 0 var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius-sm);
    background: transparent;
    color: var(--k-fg-muted);
    font: 600 var(--k-font-size-xs) var(--k-font-mono);
    cursor: pointer;
  }

  .tool:hover,
  .tool:focus-visible {
    background: var(--k-bg-hover);
    color: var(--k-fg);
  }

  .tool.missing {
    opacity: 0.5;
  }
</style>
