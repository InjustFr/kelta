<script lang="ts">
  // Welcome / first-run pane: a three-step guide, then the environment checks Kelta works with.
  // Data comes from `diagnostics_run` (on demand, once per mount).
  import { dispatch } from '$lib/actions';
  import type { PaneProps } from '$app/registry';
  import type { Check } from '$lib/gen';
  import { diagnosticsRun } from '$lib/ipc/commands';
  import { toasts, ui } from '$lib/stores';
  import { Badge, Button, Icon, Kbd, Spinner } from '$lib/ui';

  import { chordFor } from '../../shell/labels';
  import { spawnTemplate } from '../../shell/spawn';

  let { projectId }: Pick<PaneProps<'welcome'>, 'projectId'> = $props();

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
  const RANK = { fail: 0, warn: 1, ok: 2 } as const;

  const sorted = $derived(checks ? [...checks].sort((a, b) => RANK[a.status] - RANK[b.status]) : []);
  const missing = $derived(sorted.filter((c) => c.status !== 'ok').length);
  const paletteChord = chordFor('palette.open');

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
    <p class="lead">
      Run Claude Code and nvim side by side, with your tickets and pull requests next to them. Three steps to
      get going.
    </p>
  </header>

  <ol class="steps">
    <li>
      <span class="num" aria-hidden="true">1</span>
      <div class="step">
        <h2>Add your first project</h2>
        <p>Pick a code folder. Kelta finds its git repositories, remotes and tracker.</p>
        <div class="act">
          <Button variant="primary" icon="folder-plus" onclick={() => ui.openSheet('project_new')}>
            Create project from folder
          </Button>
        </div>
      </div>
    </li>
    <li>
      <span class="num" aria-hidden="true">2</span>
      <div class="step">
        <h2>Connect your tracker and code host <Badge>Optional</Badge></h2>
        <p>
          Jira, Linear, GitHub, GitLab or Redmine. Shows your tickets and pull requests next to the code.
          Tokens stay in your system keychain.
        </p>
        <div class="act">
          <Button onclick={() => openSettings('accounts')}>Add account</Button>
        </div>
      </div>
    </li>
    <li>
      <span class="num" aria-hidden="true">3</span>
      <div class="step">
        <h2>Start Claude and nvim</h2>
        <p>Once your project is added, one click opens Claude Code and nvim side by side in its folder.</p>
      </div>
    </li>
  </ol>

  <section class="env" aria-label="Environment">
    <h2>Your environment</h2>
    {#if error}
      <p class="error" role="alert">{error}</p>
      <Button icon="refresh-cw" onclick={run}>Check again</Button>
    {:else if !checks}
      <div class="wait"><Spinner size={16} /></div>
    {:else}
      <p class="summary">
        {missing === 0
          ? 'Everything Kelta uses was found.'
          : `${missing} missing. Kelta still runs; only the features that need them are off.`}
      </p>
      <ul>
        {#each sorted as c (c.id)}
          <li class={c.status}>
            <Icon name={ICONS[c.status]} size={16} />
            <span class="label">{c.label}</span>
            <span class="detail">{c.detail}</span>
            {#if c.fix}<code class="fix">{c.fix}</code>{/if}
          </li>
        {/each}
      </ul>
      <Button icon="refresh-cw" onclick={run}>Check again</Button>
    {/if}
  </section>

  <footer>
    <p>
      Just want a terminal?
      <Button variant="ghost" size="sm" onclick={() => spawnTemplate(projectId, 'shell')}>
        Open a terminal
      </Button>
    </p>
    {#if paletteChord}
      <p>Press <Kbd chord={paletteChord} /> any time to search commands, projects and tickets.</p>
    {/if}
  </footer>
</div>

<style>
  .welcome {
    height: 100%;
    overflow: auto;
    box-sizing: border-box;
    max-width: 640px;
    padding: var(--k-space-8) var(--k-space-6);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-7);
  }

  h1 {
    margin: 0 0 var(--k-space-3);
    font-size: var(--k-font-size-display);
    font-weight: var(--k-weight-strong);
    line-height: 1.2;
  }

  h2 {
    margin: 0;
    font-size: var(--k-font-size-lg);
    font-weight: var(--k-weight-strong);
  }

  p {
    margin: 0;
  }

  .lead,
  .step p,
  .summary,
  footer {
    color: var(--k-fg-muted);
    line-height: var(--k-line-height-read);
  }

  .lead {
    max-width: 52ch;
  }

  /* The steps are a real sequence: a thin line threads the numbers together. */
  .steps {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .steps > li {
    position: relative;
    display: grid;
    grid-template-columns: 24px 1fr;
    column-gap: var(--k-space-4);
    padding-bottom: var(--k-space-6);
  }

  .steps > li:last-child {
    padding-bottom: 0;
  }

  .steps > li:not(:last-child)::after {
    content: '';
    position: absolute;
    left: 11.5px;
    top: 30px;
    bottom: var(--k-space-2);
    width: 1px;
    background: var(--k-border-strong);
    opacity: 0.5;
  }

  .num {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--k-border-strong);
    background: var(--k-bg-float);
    color: var(--k-fg);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-strong);
  }

  .step {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .step h2 {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-3);
    min-height: 24px;
  }

  .step p {
    max-width: 52ch;
  }

  .act {
    margin-top: var(--k-space-3);
  }

  .env ul {
    margin: var(--k-space-3) 0 var(--k-space-4);
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  .env h2 {
    margin-bottom: var(--k-space-2);
  }

  .env li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--k-space-3);
  }

  .env li :global(.k-icon) {
    align-self: center;
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
    min-width: 120px;
    font-weight: var(--k-weight-strong);
  }

  .detail {
    color: var(--k-fg-muted);
  }

  .fix {
    margin-left: auto;
    padding: 1px 6px;
    border-radius: var(--k-radius-sm);
    background: var(--k-bg-sunken);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xs);
  }

  footer {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  footer p {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
  }

  .error {
    color: var(--k-danger);
  }

  .wait {
    padding: var(--k-space-4);
  }
</style>
