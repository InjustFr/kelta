<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
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
        installed ? t('Removing Kelta from Claude Code failed') : t('Installing Kelta in Claude Code failed'),
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
      toasts.error(err, hooks ? t("Removing Kelta's hooks failed") : t("Installing Kelta's hooks failed"));
    } finally {
      hooksBusy = false;
    }
  }
</script>

<SectionForm sectionId="claude" {...props}>
  {#snippet before()}
    {#if installed !== undefined}
      <SettingsRow label={t("Kelta's ticket tools in Claude Code")} actions data-testid="claude-mcp">
        {#snippet control()}
          {#if installed !== null}
            <Button size="sm" icon={installed ? 'trash' : 'plug'} loading={busy} onclick={toggle}>
              {installed ? t('Remove') : t('Install')}
            </Button>
          {/if}
        {/snippet}
        <p>
          {#if installed === null}
            {t('Claude Code was not found in your login PATH.')}
          {:else if installed}
            {t("Kelta's ticket tools are installed in Claude Code: every Claude session can use them.")}
          {:else}
            {t("Install Kelta's ticket tools in Claude Code to use them from any Claude session.")}
          {/if}
        </p>
      </SettingsRow>
    {/if}
    {#if hooks !== undefined && hooks !== null}
      <SettingsRow label={t('Claude sessions started outside Kelta')} actions data-testid="claude-hooks">
        {#snippet control()}
          <Button size="sm" icon={hooks ? 'trash' : 'plug'} loading={hooksBusy} onclick={toggleHooks}>
            {hooks ? t('Remove') : t('Install')}
          </Button>
        {/snippet}
        <p>
          {#if hooks}
            {t('Claude sessions you start in another terminal show in the Queue, with their state.')}
          {:else}
            {t(
              "Install Kelta's hooks in Claude Code's user settings to show the Claude sessions you start in another terminal in the Queue.",
            )}
          {/if}
        </p>
      </SettingsRow>
    {/if}
    {#if mode !== undefined}
      <SettingsRow
        label={t('Profiles without a permission mode start in')}
        actions
        data-testid="claude-permission-mode"
      >
        {#snippet control()}
          <span>{mode ? t('last used: {mode}', { mode }) : t('Claude default')}</span>
        {/snippet}
      </SettingsRow>
    {/if}
  {/snippet}
</SectionForm>
