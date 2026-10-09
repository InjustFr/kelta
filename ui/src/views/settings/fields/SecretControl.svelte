<script lang="ts">
  // SecretRef editor: source picker + "Set token…" + "Test". The token value is only ever held in
  // a local password input, sent through `secret_set`, and cleared; it is never rendered back.
  import type { AccountTestResult } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import Select from '$lib/ui/Select.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  interface Props {
    /** Current SecretRef string (`keyring:jira-acme`, `gh-cli`, …) or empty. */
    value: string;
    /** Called with the new SecretRef (or null to clear). */
    onchange: (ref: string | null) => void;
    /** Account being edited (enables Test and proposes a keyring name). */
    accountId?: string | null;
    /** Account kind: restricts the CLI sources (`gh-cli` for github, `glab-cli` for gitlab). */
    accountKind?: string | null;
    readonly?: boolean;
    /** Test hook: runs before Test (the account must be saved first). */
    onbeforetest?: () => Promise<void> | void;
  }

  let {
    value,
    onchange,
    accountId = null,
    accountKind = null,
    readonly = false,
    onbeforetest,
  }: Props = $props();

  type Kind = 'none' | 'keyring' | 'gh-cli' | 'glab-cli' | 'command' | 'env';

  function parse(ref: string): { kind: Kind; arg: string } {
    if (!ref) return { kind: 'none', arg: '' };
    if (ref === 'gh-cli' || ref === 'glab-cli') return { kind: ref, arg: '' };
    const i = ref.indexOf(':');
    if (i > 0) {
      const k = ref.slice(0, i);
      if (k === 'keyring' || k === 'command' || k === 'env') return { kind: k, arg: ref.slice(i + 1) };
    }
    return { kind: 'command', arg: ref };
  }

  let kind = $state<Kind>('none');
  let arg = $state('');
  let token = $state('');
  let tokenOpen = $state(false);
  let busy = $state(false);
  let testResult = $state<AccountTestResult | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const p = parse(value);
    kind = p.kind;
    arg = p.arg;
  });

  const options = $derived([
    { value: 'keyring', label: 'OS keychain (keyring)' },
    ...(accountKind === 'github' || accountKind === null
      ? [{ value: 'gh-cli', label: 'GitHub CLI (gh)' }]
      : []),
    ...(accountKind === 'gitlab' || accountKind === null
      ? [{ value: 'glab-cli', label: 'GitLab CLI (glab)' }]
      : []),
    { value: 'command', label: 'Command (pass, op, secret-tool…)' },
    { value: 'env', label: 'Environment variable' },
    { value: 'none', label: kind === 'none' ? 'Not set' : 'Clear' },
  ] as { value: Kind; label: string }[]);

  function build(k: Kind, a: string): string | null {
    if (k === 'none') return null;
    if (k === 'gh-cli' || k === 'glab-cli') return k;
    return a.trim() ? `${k}:${a.trim()}` : null;
  }

  function pick(k: Kind): void {
    // the old argument belongs to the old source: never carry it over
    if (k !== kind) arg = k === 'keyring' ? (accountId ?? '') : '';
    kind = k;
    const ref = build(k, arg);
    if (ref !== null || k === 'none') onchange(ref);
  }

  function commitArg(): void {
    const ref = build(kind, arg);
    if (ref !== null) onchange(ref);
  }

  async function saveToken(): Promise<void> {
    const ref = build('keyring', arg);
    if (!ref || !token) return;
    busy = true;
    error = null;
    try {
      await ipc.secretSet({ secret_ref: ref, value: token });
      token = '';
      tokenOpen = false;
      if (value !== ref) onchange(ref);
      toasts.info('Token stored in the keychain');
    } catch (err) {
      error = toIpcError('secret_set', err).message;
    } finally {
      busy = false;
      token = '';
    }
  }

  async function test(): Promise<void> {
    if (!accountId) return;
    busy = true;
    error = null;
    testResult = null;
    try {
      await onbeforetest?.();
      testResult = await ipc.accountTest({ account_id: accountId });
    } catch (err) {
      error = toIpcError('account_test', err).message;
    } finally {
      busy = false;
    }
  }
</script>

<div class="secret" data-testid="secret-control">
  <div class="line">
    <Select
      value={kind}
      {options}
      disabled={readonly}
      onchange={(k) => pick(k as Kind)}
      id={accountId ? `secret-kind-${accountId}` : undefined}
    />
    {#if kind === 'keyring' || kind === 'command' || kind === 'env'}
      <TextInput
        bind:value={arg}
        disabled={readonly}
        aria-label="Secret reference argument"
        placeholder={kind === 'keyring' ? 'name' : kind === 'env' ? 'VARIABLE' : 'pass show jira/acme'}
        onchange={commitArg}
      />
    {/if}
    {#if kind === 'keyring' && !readonly}
      <Button size="sm" onclick={() => (tokenOpen = !tokenOpen)}>Set token…</Button>
    {/if}
    {#if accountId}
      <Button size="sm" loading={busy && !tokenOpen} onclick={test} disabled={!value}>Test</Button>
    {/if}
  </div>
  {#if tokenOpen}
    <form
      class="line"
      onsubmit={(e) => {
        e.preventDefault();
        void saveToken();
      }}
    >
      <TextInput
        bind:value={token}
        type="password"
        autocomplete="off"
        aria-label="Token"
        placeholder="Paste the token (stored in the OS keychain, never in config.toml)"
      />
      <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!token || !arg.trim()}
        >Save</Button
      >
    </form>
  {/if}
  {#if testResult}
    <p class="result" class:ok={testResult.ok} role="status" data-testid="account-test-result">
      {#if testResult.ok}
        Connected{testResult.user ? ` as ${testResult.user.name}` : ''}.
      {:else}
        {testResult.error?.message ?? 'Connection failed'}
      {/if}
    </p>
  {/if}
  {#if error}<p class="result" role="alert">{error}</p>{/if}
</div>

<style>
  .secret {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    flex-wrap: wrap;
  }

  .result {
    margin: 0;
    font-size: var(--k-font-size-sm);
    color: var(--k-danger);
  }

  .result.ok {
    color: var(--k-ok);
  }
</style>
