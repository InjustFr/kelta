<script lang="ts">
  // `window.decorations = "custom"` (ARCH §12, SPEC §8): the system title bar is gone, so the app
  // draws a drag strip and edge/corner resize handles. Renders nothing in the other modes.
  import { onMount } from 'svelte';

  import { appInfo } from '$lib/ipc/commands';
  import { IconButton } from '$lib/ui';

  type Dir = 'North' | 'South' | 'East' | 'West' | 'NorthEast' | 'NorthWest' | 'SouthEast' | 'SouthWest';
  const HANDLES: Dir[] = [
    'North',
    'South',
    'East',
    'West',
    'NorthEast',
    'NorthWest',
    'SouthEast',
    'SouthWest',
  ];

  let custom = $state(false);

  onMount(() => {
    appInfo()
      .then((info) => (custom = info.decorations === 'custom'))
      .catch(() => {});
  });

  async function win(op: 'minimize' | 'toggleMaximize' | 'close'): Promise<void> {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow()[op]();
  }

  async function resize(e: PointerEvent, dir: Dir): Promise<void> {
    if (e.button !== 0) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow().startResizeDragging(dir);
  }
</script>

{#if custom}
  <!-- Undecorated window: a bezel drag strip with the only window controls (no system title bar). -->
  <div class="titlebar" data-tauri-drag-region data-testid="titlebar">
    <span class="controls">
      <IconButton icon="minus" label="Minimize" size="sm" onclick={() => void win('minimize')} />
      <IconButton icon="square" label="Maximize" size="sm" onclick={() => void win('toggleMaximize')} />
      <IconButton icon="x" label="Close window" size="sm" onclick={() => void win('close')} />
    </span>
  </div>
  {#each HANDLES as dir (dir)}
    <div class="handle {dir}" aria-hidden="true" onpointerdown={(e) => void resize(e, dir)}></div>
  {/each}
{/if}

<style>
  .titlebar {
    flex: none;
    height: var(--k-statusbar-height);
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: 0 var(--k-space-2);
    background: var(--k-bezel);
    user-select: none;
  }

  .controls {
    display: inline-flex;
    gap: var(--k-space-1);
  }
  .handle {
    position: fixed;
    z-index: var(--k-z-toast);
  }
  .North,
  .South {
    left: 8px;
    right: 8px;
    height: 4px;
    cursor: ns-resize;
  }
  .East,
  .West {
    top: 8px;
    bottom: 8px;
    width: 4px;
    cursor: ew-resize;
  }
  .North {
    top: 0;
  }
  .South {
    bottom: 0;
  }
  .East {
    right: 0;
  }
  .West {
    left: 0;
  }
  .NorthEast,
  .NorthWest,
  .SouthEast,
  .SouthWest {
    width: 8px;
    height: 8px;
  }
  .NorthWest {
    top: 0;
    left: 0;
    cursor: nwse-resize;
  }
  .SouthEast {
    bottom: 0;
    right: 0;
    cursor: nwse-resize;
  }
  .NorthEast {
    top: 0;
    right: 0;
    cursor: nesw-resize;
  }
  .SouthWest {
    bottom: 0;
    left: 0;
    cursor: nesw-resize;
  }
</style>
