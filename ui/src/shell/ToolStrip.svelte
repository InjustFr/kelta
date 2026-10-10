<script lang="ts">
  // The active project's first tools, in config order, as labelled buttons at the right end of the
  // tab bar (the New tab menu lists more). Pressed while the tool runs in this work item; reads the
  // cached tool list and the session and layout stores: no polling, no timers.
  import { dispatch } from '$lib/actions';
  import type { ProjectId, ToolInfo, WorkItemId } from '$lib/gen';
  import { tools } from '$lib/stores';
  import { Button, isIconName } from '$lib/ui';
  import { formatChord } from '$lib/ui/format';

  import { liveToolPane } from '../views/tools/actions';

  let { projectId, workItemId }: { projectId: ProjectId; workItemId: WorkItemId | null } = $props();

  const list = $derived(tools.list(projectId).slice(0, 6));

  // External apps are not tracked, so they never show as running.
  const running = (t: ToolInfo): boolean =>
    t.kind !== 'external' && liveToolPane(projectId, t.id, workItemId) !== null;

  function hint(t: ToolInfo, live: boolean): string {
    if (t.installed === false) return 'Not installed';
    const chord = t.keybinding ? ` (${formatChord(t.keybinding).join('')})` : '';
    return `${t.label}${chord}${live ? ' - running' : ''}`;
  }
</script>

{#if list.length > 0}
  <div class="strip" role="toolbar" aria-label="Tools" data-testid="tool-strip">
    {#each list as t (t.id)}
      {@const live = running(t)}
      <Button
        variant="ghost"
        size="sm"
        icon={t.icon && isIconName(t.icon) ? t.icon : undefined}
        class="tool {live ? 'pressed' : ''} {t.installed === false ? 'missing' : ''}"
        title={hint(t, live)}
        aria-pressed={live}
        data-tool-id={t.id}
        onclick={() => void dispatch('tools.open', { tool_id: t.id })}>{t.label}</Button
      >
    {/each}
  </div>
{/if}

<style>
  /* The tab bar scrolls sideways; sticky keeps the tools in view when tabs overflow. */
  .strip {
    position: sticky;
    right: 0;
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    padding-left: var(--k-space-3);
    background: var(--k-bezel);
  }

  .strip :global(.tool) {
    color: var(--k-fg-chrome);
  }

  /* The active tab's well face. */
  .strip :global(.tool.pressed) {
    background: var(--k-well);
    box-shadow: 0 0 0 1px var(--k-border);
    color: var(--k-fg);
  }

  .strip :global(.tool.missing) {
    opacity: 0.55;
  }
</style>
