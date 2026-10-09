<script lang="ts">
  import type { SettingsSectionProps } from '$app/registry';
  import type { Check } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

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
      toasts.error(err, 'Test notification failed');
    } finally {
      testing = false;
    }
  }
</script>

<SectionForm sectionId="notifications" {...props}>
  {#snippet before()}
    {#if daemon}
      <div class="warn" role="status" data-testid="notification-daemon-warning">
        <Icon name="triangle-alert" size={16} />
        <div>
          <strong>{daemon.label}</strong>
          <p>{daemon.detail}</p>
          {#if daemon.fix}<p class="fix">{daemon.fix}</p>{/if}
          <p>In-app toasts still work.</p>
        </div>
      </div>
    {/if}
    <div class="test">
      <Button size="sm" icon="bell" loading={testing} onclick={test}>Send a test notification</Button>
    </div>
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

  .test {
    margin-bottom: var(--k-space-3);
  }
</style>
