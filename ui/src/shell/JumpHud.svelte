<script lang="ts">
  // Mod+J's HUD (ticket #136). With nothing waiting, `Enter` opens the offered view while it shows.
  import { dispatch } from '$lib/actions';
  import { Kbd } from '$lib/ui';

  import { hud } from './hud.svelte';

  $effect(() => {
    const enter = hud.enter;
    if (!enter) return;
    // Capture: the focused terminal must not see this Enter.
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== 'Enter' || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      e.preventDefault();
      e.stopPropagation();
      hud.hide();
      void dispatch(enter.action);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });
</script>

{#if hud.text}
  <div class="hud" role="status" data-testid="jump-hud">
    <span>{hud.text}</span>
    {#if hud.enter}<span class="enter"><Kbd chord="enter" /> {hud.enter.label}</span>{/if}
  </div>
{/if}

<style>
  .hud {
    position: fixed;
    top: var(--k-space-5);
    left: 50%;
    transform: translateX(-50%);
    z-index: var(--k-z-toast);
    display: flex;
    gap: var(--k-space-4);
    align-items: center;
    padding: var(--k-space-2) var(--k-space-4);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
    pointer-events: none;
  }

  .enter {
    color: var(--k-fg-muted);
  }
</style>
