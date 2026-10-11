<script lang="ts">
  import type { SettingsSectionProps } from '$app/registry';
  import type { PermissionMode } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';

  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  // Kelta's MCP in Claude Code (user scope): undefined = loading, null = Claude Code not found.
  let installed = $state<boolean | null | undefined>(undefined);
  let busy = $state(false);
  // Mode of sessions whose profile sets none (#277): undefined = loading, null = Claude's default.
  let mode = $state<PermissionMode | null | undefined>(undefined);

  $effect(() => {
    void ipc
      .claudeMcpStatus()
      .then((s) => (installed = s))
      .catch(() => (installed = null));
    void ipc
      .claudePermissionMode()
      .then((m) => (mode = m))
      .catch(() => (mode = null));
  });

  async function toggle(): Promise<void> {
    busy = true;
    try {
      if (installed) await ipc.claudeMcpRemove();
      else await ipc.claudeMcpInstall();
      installed = !installed;
    } catch (err) {
      toasts.error(
        err,
        installed ? 'Removing Kelta from Claude Code failed' : 'Installing Kelta in Claude Code failed',
      );
    } finally {
      busy = false;
    }
  }
</script>

<SectionForm sectionId="claude" {...props}>
  {#snippet before()}
    {#if installed !== undefined}
      <div class="mcp" data-testid="claude-mcp">
        <p>
          {#if installed === null}
            Claude Code was not found in your login PATH.
          {:else if installed}
            Kelta's ticket tools are installed in Claude Code: every Claude session can use them.
          {:else}
            Install Kelta's ticket tools in Claude Code to use them from any Claude session.
          {/if}
        </p>
        {#if installed !== null}
          <Button size="sm" icon={installed ? 'trash-2' : 'plug'} loading={busy} onclick={toggle}>
            {installed ? 'Remove' : 'Install'}
          </Button>
        {/if}
      </div>
    {/if}
    {#if mode !== undefined}
      <p class="mode" data-testid="claude-permission-mode">
        Profiles without a permission mode start in: {mode ? `last used: ${mode}` : 'Claude default'}
      </p>
    {/if}
  {/snippet}
</SectionForm>

<style>
  .mcp {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    margin-bottom: var(--k-space-3);
  }

  .mcp p {
    flex: 1;
    margin: 0;
    color: var(--k-fg-muted);
  }

  .mode {
    margin: 0 0 var(--k-space-3);
    color: var(--k-fg-muted);
  }
</style>
