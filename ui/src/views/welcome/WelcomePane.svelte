<script lang="ts">
  // Welcome / first-run pane (SPEC §5): detects the tools Kelta works with and offers the first
  // steps. Data comes from `diagnostics_run` (on demand, once per mount).
  import { dispatch } from '$lib/actions';
  import type { PaneProps } from '$app/registry';
  import type { Check } from '$lib/gen';
  import { diagnosticsRun } from '$lib/ipc/commands';
  import { toasts, ui } from '$lib/stores';
  import { Button, Icon, Spinner } from '$lib/ui';

  let { projectId }: PaneProps<'welcome'> = $props();

  let checks = $state<Check[] | null>(null);
  let error = $state<string | null>(null);

  async function run(): Promise<void> {
    checks = null;
    error = null;
    try {
      checks = (await diagnosticsRun()).checks;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  $effect(() => {
    void run();
  });

  const ICONS = { ok: 'circle-check', warn: 'triangle-alert', fail: 'circle-x' } as const;

  async function openSettings(section: string): Promise<void> {
    try {
      const handled = await dispatch('settings.open', { section, project_id: projectId });
      if (!handled) toasts.warn('Settings are not available yet');
    } catch (err) {
      toasts.error(err, 'Could not open settings');
    }
  }
</script>

<div class="welcome" data-testid="welcome-pane">
  <header>
    <h1>Welcome to Kelta</h1>
    <p>Claude Code and your editor side by side, tickets, reviews and tools in one window.</p>
  </header>

  <section aria-label="Environment">
    <h2>Your environment</h2>
    {#if error}
      <p class="error" role="alert">{error}</p>
      <Button icon="refresh-cw" onclick={run}>Check again</Button>
    {:else if !checks}
      <div class="wait"><Spinner size={16} /></div>
    {:else}
      <ul>
        {#each checks as c (c.id)}
          <li class={c.status}>
            <Icon name={ICONS[c.status]} size={15} />
            <span class="label">{c.label}</span>
            <span class="detail">{c.detail}</span>
            {#if c.fix}<code class="fix">{c.fix}</code>{/if}
          </li>
        {/each}
      </ul>
      <Button icon="refresh-cw" onclick={run}>Check again</Button>
    {/if}
  </section>

  <section class="actions" aria-label="Get started">
    <h2>Get started</h2>
    <div class="buttons">
      <Button variant="primary" icon="folder-plus" onclick={() => ui.openSheet('project_new')}>
        Create project from folder
      </Button>
      <Button icon="key-round" onclick={() => openSettings('accounts')}>Add account</Button>
      <Button icon="play" onclick={() => ui.openSheet('onboarding')}>Run setup</Button>
      <Button icon="square-terminal" onclick={() => dispatch('session.new')}>New session</Button>
    </div>
  </section>
</div>

<style>
  .welcome {
    height: 100%;
    overflow: auto;
    padding: var(--k-space-6);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-5);
    max-width: 760px;
  }

  h1 {
    margin: 0 0 var(--k-space-2);
    font-size: 20px;
  }

  h2 {
    margin: 0 0 var(--k-space-3);
    font-size: var(--k-font-size-lg);
  }

  header p {
    margin: 0;
    color: var(--k-fg-muted);
  }

  ul {
    margin: 0 0 var(--k-space-4);
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  li {
    display: flex;
    align-items: baseline;
    gap: var(--k-space-3);
  }

  li.ok :global(.k-icon) {
    color: var(--k-ok);
  }

  li.warn :global(.k-icon) {
    color: var(--k-warn);
  }

  li.fail :global(.k-icon) {
    color: var(--k-danger);
  }

  .label {
    min-width: 130px;
    font-weight: 600;
  }

  .detail {
    color: var(--k-fg-muted);
  }

  .fix {
    margin-left: auto;
    padding: 1px 6px;
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    font-size: var(--k-font-size-xs);
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--k-space-3);
  }

  .error {
    color: var(--k-danger);
  }

  .wait {
    padding: var(--k-space-4);
  }
</style>
