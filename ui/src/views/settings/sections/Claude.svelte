<script lang="ts">
  import type { SettingsSectionProps } from '$app/registry';
  import type { PermissionMode } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';

  import SettingsRow from '../layout/SettingsRow.svelte';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  // Kelta's MCP in Claude Code (user scope): undefined = loading, null = Claude Code not found.
  let installed = $state<boolean | null | undefined>(undefined);
  let busy = $state(false);
  // Kelta's hook relay in Claude Code's user settings (#345), same states.
  let hooks = $state<boolean | null | undefined>(undefined);
  let hooksBusy = $state(false);
  // Mode of sessions whose profile sets none (#277): undefined = loading, null = Claude's default.
  let mode = $state<PermissionMode | null | undefined>(undefined);

  $effect(() => {
    void ipc
      .claudeMcpStatus()
      .then((s) => (installed = s))
      .catch(() => (installed = null));
    void ipc
      .claudeHooksStatus()
      .then((s) => (hooks = s))
      .catch(() => (hooks = null));
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

  async function toggleHooks(): Promise<void> {
    hooksBusy = true;
    try {
      if (hooks) await ipc.claudeHooksRemove();
      else await ipc.claudeHooksInstall();
      hooks = !hooks;
    } catch (err) {
      toasts.error(err, hooks ? "Removing Kelta's hooks failed" : "Installing Kelta's hooks failed");
    } finally {
      hooksBusy = false;
    }
  }
</script>

<SectionForm sectionId="claude" {...props}>
  {#snippet before()}
    {#if installed !== undefined}
      <SettingsRow label="Kelta's ticket tools in Claude Code" actions data-testid="claude-mcp">
        {#snippet control()}
          {#if installed !== null}
            <Button size="sm" icon={installed ? 'trash-2' : 'plug'} loading={busy} onclick={toggle}>
              {installed ? 'Remove' : 'Install'}
            </Button>
          {/if}
        {/snippet}
        <p>
          {#if installed === null}
            Claude Code was not found in your login PATH.
          {:else if installed}
            Kelta's ticket tools are installed in Claude Code: every Claude session can use them.
          {:else}
            Install Kelta's ticket tools in Claude Code to use them from any Claude session.
          {/if}
        </p>
      </SettingsRow>
    {/if}
    {#if hooks !== undefined && hooks !== null}
      <SettingsRow label="Claude sessions started outside Kelta" actions data-testid="claude-hooks">
        {#snippet control()}
          <Button size="sm" icon={hooks ? 'trash-2' : 'plug'} loading={hooksBusy} onclick={toggleHooks}>
            {hooks ? 'Remove' : 'Install'}
          </Button>
        {/snippet}
        <p>
          {#if hooks}
            Claude sessions you start in another terminal show in Now, with their state.
          {:else}
            Install Kelta's hooks in Claude Code's user settings to show the Claude sessions you start in
            another terminal in Now.
          {/if}
        </p>
      </SettingsRow>
    {/if}
    {#if mode !== undefined}
      <SettingsRow
        label="Profiles without a permission mode start in"
        actions
        data-testid="claude-permission-mode"
      >
        {#snippet control()}
          <span>{mode ? `last used: ${mode}` : 'Claude default'}</span>
        {/snippet}
      </SettingsRow>
    {/if}
  {/snippet}
</SectionForm>
