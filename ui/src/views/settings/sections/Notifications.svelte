<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';
  import type { Check } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  import notificationsCopy from '../copy/notifications';
  import SettingsRow from '../layout/SettingsRow.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  const editor = useEditor(() => ({ layer: props.layer, projectId: props.projectId, repoId: props.repoId }));
  // Every other key does nothing while notifications are off (notifier.rs should_notify): shown disabled, not skipped, so search hits still land.
  const disabled = $derived(
    editor.valueOf('notifications.enabled') === false
      ? Object.keys(notificationsCopy).filter((p) => p !== 'notifications.enabled')
      : [],
  );

  let daemon = $state<Check | null>(null);
  let testing = $state(false);

  $effect(() => {
    void ipc
      .diagnosticsRun()
      .then((d) => {
        daemon = d.checks.find((c) => /notif/i.test(c.id) && c.status !== 'ok') ?? null;
      })
      .catch(() => {
        daemon = null;
      });
  });

  async function test(): Promise<void> {
    testing = true;
    try {
      await ipc.notifyTest();
    } catch (err) {
      toasts.error(err, t('Test notification failed'));
    } finally {
      testing = false;
    }
  }
</script>

<SectionForm sectionId="notifications" {...props} {disabled}>
  {#snippet before()}
    {#if daemon}
      <div class="warn" role="status" data-testid="notification-daemon-warning">
        <Icon name="triangle-alert" size={16} />
        <div>
          <strong>{daemon.label}</strong>
          <p>{daemon.detail}</p>
          {#if daemon.fix}<p class="fix">{daemon.fix}</p>{/if}
          <p>{t('In-app toasts still work.')}</p>
        </div>
      </div>
    {/if}
  {/snippet}
  {#snippet after()}
    <SettingsRow label={t('Test notification')} actions data-testid="notification-test">
      {#snippet control()}
        <Button size="sm" icon="bell" loading={testing} onclick={test}>{t('Send a test notification')}</Button
        >
      {/snippet}
      <p>{t('Checks that desktop notifications reach you.')}</p>
      <p>{t('Nothing shows? Allow Kelta in your system’s notification settings.')}</p>
    </SettingsRow>
  {/snippet}
</SectionForm>

<style>
  .warn {
    display: flex;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-warn);
    border-radius: var(--k-radius);
    margin-bottom: var(--k-space-3);
  }

  .warn p {
    margin: var(--k-space-1) 0 0;
    color: var(--k-fg-muted);
  }

  .fix {
    font-family: var(--k-font-mono);
  }
</style>
