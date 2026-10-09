<script lang="ts">
  // First run (SPEC §5): dependency checks (claude, nvim, git, gh, glab, secret backend,
  // notification daemon), then the two first steps: a project and an account.
  import type { SheetProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { AppInfo, Diagnostics } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, settings, ui } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Sheet from '$lib/ui/Sheet.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  import { openDiagnostics } from '../diagnostics/open';
  import { needsAttention, sortChecks, STATUS_ICON, STATUS_TONE } from '../diagnostics/report';

  let { onclose }: SheetProps = $props();

  let info = $state<AppInfo | null>(null);
  let diag = $state<Diagnostics | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function run(): Promise<void> {
    loading = true;
    error = null;
    try {
      [info, diag] = await Promise.all([ipc.appInfo(), ipc.diagnosticsRun()]);
    } catch (err) {
      error = toIpcError('diagnostics_run', err).message;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void run();
    if (settings.value() === null) void settings.load().catch(() => undefined);
  });

  const hasProject = $derived(projects.list.some((p) => !p.builtin));
  const hasAccount = $derived(Object.keys(settings.value()?.accounts ?? {}).length > 0);
  const checks = $derived(diag ? sortChecks(diag.checks) : []);

  function finish(): void {
    try {
      localStorage.setItem('kelta.onboarding.done', '1');
    } catch {
      // storage can be unavailable; the sheet simply shows again next time
    }
    onclose();
  }

  function createProject(): void {
    ui.closeSheet('onboarding');
    ui.openSheet('project_new');
  }

  async function addAccount(): Promise<void> {
    ui.closeSheet('onboarding');
    await dispatch('settings.open', { section: 'accounts' });
  }

  async function showDiagnostics(): Promise<void> {
    ui.closeSheet('onboarding');
    await openDiagnostics();
  }
</script>

<Sheet title="Welcome to Kelta" onclose={finish} width={560}>
  <div class="welcome" data-testid="onboarding">
    <p class="lead">
      Claude Code and your editor side by side, with your tickets and reviews one click away. A quick look at
      your machine first.
    </p>

    <section>
      <header>
        <h3>Your environment</h3>
        <Button size="sm" icon="refresh-cw" {loading} onclick={run}>Check again</Button>
      </header>
      {#if error}
        <p class="error" role="alert">{error}</p>
      {:else if !diag}
        <div class="loading"><Spinner size={16} /> Checking…</div>
      {:else}
        <ul>
          {#each checks as c (c.id)}
            <li data-testid="check" data-check-id={c.id} data-status={c.status}>
              <span class="icon {c.status}"><Icon name={STATUS_ICON[c.status]} size={16} /></span>
              <div>
                <strong>{c.label}</strong>
                <Badge tone={STATUS_TONE[c.status]}>{c.status}</Badge>
                <p>{c.detail}</p>
                {#if c.fix}<p class="fix"><code>{c.fix}</code></p>{/if}
              </div>
            </li>
          {/each}
        </ul>
        {#if info?.claude && !info.claude.ok}
          <p class="error" data-testid="claude-too-old">
            Claude Code {info.claude.version} is older than the minimum Kelta supports. Update it to get status
            hooks and session names.
          </p>
        {/if}
        {#if needsAttention(diag)}
          <p class="muted">
            Kelta works with what is missing; those features are just unavailable. Details stay in
            Diagnostics.
          </p>
        {/if}
      {/if}
    </section>

    <section>
      <h3>First steps</h3>
      <div class="steps">
        <div class="step" data-done={hasProject}>
          <Icon name={hasProject ? 'circle-check' : 'folder-plus'} size={18} />
          <div>
            <strong>Create a project from a folder</strong>
            <p>Kelta detects the repositories, remotes and tracker.</p>
          </div>
          <Button variant={hasProject ? 'secondary' : 'primary'} onclick={createProject}
            >Create project from folder</Button
          >
        </div>
        <div class="step" data-done={hasAccount}>
          <Icon name={hasAccount ? 'circle-check' : 'key-round'} size={18} />
          <div>
            <strong>Add an account</strong>
            <p>Jira, Redmine, GitHub or GitLab. Tokens go to your system keychain.</p>
          </div>
          <Button variant={hasAccount ? 'secondary' : 'primary'} onclick={addAccount}>Add account</Button>
        </div>
      </div>
    </section>
  </div>

  {#snippet actions()}
    <Button variant="ghost" onclick={showDiagnostics}>Open diagnostics</Button>
    <Button variant="primary" onclick={finish} data-testid="onboarding-done">Done</Button>
  {/snippet}
</Sheet>

<style>
  .welcome {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-5);
  }

  .lead {
    margin: 0;
    color: var(--k-fg-muted);
  }

  h3 {
    margin: 0 0 var(--k-space-3);
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  li {
    display: flex;
    gap: var(--k-space-3);
  }

  li p,
  .step p {
    margin: var(--k-space-1) 0 0;
    color: var(--k-fg-muted);
    overflow-wrap: anywhere;
  }

  .icon.ok {
    color: var(--k-ok);
  }

  .icon.warn {
    color: var(--k-warn);
  }

  .icon.fail {
    color: var(--k-danger);
  }

  code {
    font-family: var(--k-font-mono);
  }

  .loading {
    display: flex;
    gap: var(--k-space-3);
    align-items: center;
    color: var(--k-fg-muted);
  }

  .error {
    color: var(--k-danger);
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .steps {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .step {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .step > div {
    flex: 1;
  }

  .step[data-done='true'] {
    opacity: 0.8;
  }
</style>
