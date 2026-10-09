<script lang="ts">
  // Accounts: wizard (kind → connection → token → test), per-account Test, backend status with
  // fixes, and the full per-account field list. Accounts are global-only settings.
  import type { SettingsSectionProps } from '$app/registry';
  import type { AccountKind, AccountTestResult, SecretBackendStatus } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { settings as settingsStore, toasts } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  import Field from '../fields/Field.svelte';
  import SecretControl from '../fields/SecretControl.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import {
    KINDS,
    backendAdvice,
    buildAccount,
    emptyDraft,
    kindInfo,
    needsEmail,
    suggestId,
    validateDraft,
    type AccountDraft,
  } from '../lib/accounts';
  import { joinPath, isRecord } from '../lib/paths';
  import { childEntries, entrySchema, nodeAt } from '../lib/schema';

  let props: SettingsSectionProps = $props();
  const editor = useEditor(() => ({ layer: props.layer, projectId: props.projectId, repoId: props.repoId }));

  const accounts = $derived.by((): { id: string; kind: AccountKind; base_url: string | null }[] => {
    const v = editor.valueOf('accounts');
    if (!isRecord(v)) return [];
    return Object.entries(v).map(([id, a]) => ({
      id,
      kind: (isRecord(a) ? a.kind : 'github') as AccountKind,
      base_url: isRecord(a) && typeof a.base_url === 'string' ? a.base_url : null,
    }));
  });
  const ids = $derived(accounts.map((a) => a.id));
  const globalOnly = $derived(editor.layer !== 'global');

  // ---- backends ---------------------------------------------------------------------------------
  let backends = $state<SecretBackendStatus[]>([]);
  let backendError = $state<string | null>(null);

  async function loadBackends(): Promise<void> {
    try {
      backends = await ipc.secretBackendsStatus();
      backendError = null;
      // No file yet: show the create fields straight away.
      creating = !!backends.find((b) => b.backend === 'encrypted-file')?.detail?.startsWith('not set up');
    } catch (err) {
      backendError = toIpcError('secret_backends_status', err).message;
    }
  }

  $effect(() => {
    void loadBackends();
  });

  // The encrypted secrets file: its passphrase goes to the backend once per app run and is
  // cleared here right away. A `not_found` answer means no file yet: confirm, then create it.
  let passphrase = $state('');
  let passphraseAgain = $state('');
  let creating = $state(false);
  let unlocking = $state(false);
  let unlockError = $state<string | null>(null);

  async function unlock(): Promise<void> {
    if (creating && passphrase !== passphraseAgain) {
      unlockError = 'The two passphrases differ';
      return;
    }
    unlocking = true;
    unlockError = null;
    try {
      await ipc.secretUnlock({ passphrase, create: creating });
      creating = false;
      await loadBackends();
    } catch (err) {
      const e = toIpcError('secret_unlock', err);
      if (e.code === 'not_found') creating = true;
      unlockError = e.message;
    } finally {
      passphrase = '';
      passphraseAgain = '';
      unlocking = false;
    }
  }

  const keyringBackend = $derived(
    backends.find((b) => b.backend === 'keychain' || b.backend === 'secret-service'),
  );

  // ---- tests ------------------------------------------------------------------------------------
  let results = $state<Record<string, AccountTestResult | 'running'>>({});

  async function test(id: string): Promise<void> {
    results = { ...results, [id]: 'running' };
    try {
      const r = await ipc.accountTest({ account_id: id });
      results = { ...results, [id]: r };
    } catch (err) {
      results = {
        ...results,
        [id]: {
          ok: false,
          user: null,
          error: {
            code: 'internal',
            message: toIpcError('account_test', err).message,
            detail: null,
            retry_after_ms: null,
          },
        },
      };
    }
  }

  // ---- wizard -----------------------------------------------------------------------------------
  let step = $state<0 | 1 | 2 | 3>(0);
  let draft = $state<AccountDraft>(emptyDraft('github'));
  let wizardOpen = $state(false);
  let errors = $state<Record<string, string>>({});
  let saving = $state(false);
  let wizardResult = $state<AccountTestResult | null>(null);
  let wizardError = $state<string | null>(null);
  let idTouched = false;

  function openWizard(): void {
    wizardOpen = true;
    step = 0;
    draft = emptyDraft('github');
    errors = {};
    wizardResult = null;
    wizardError = null;
    idTouched = false;
  }

  function pickKind(kind: AccountKind): void {
    draft = emptyDraft(kind);
    draft.id = suggestId(kind, draft.base_url, ids);
    idTouched = false;
    step = 1;
  }

  function onUrl(): void {
    if (!idTouched) draft.id = suggestId(draft.kind, draft.base_url, ids);
  }

  function toSecret(): void {
    const e = validateDraft({ ...draft, secret: draft.secret || 'x' }, ids);
    delete e.secret;
    errors = e;
    if (Object.keys(e).length === 0) {
      if (!draft.secret && kindInfo(draft.kind).secretDefault === 'keyring')
        draft.secret = `keyring:${draft.id}`;
      step = 2;
    }
  }

  async function save(andTest: boolean): Promise<void> {
    const e = validateDraft(draft, ids);
    errors = e;
    if (Object.keys(e).length > 0) return;
    saving = true;
    wizardError = null;
    try {
      const ok = await editor.set(joinPath(['accounts', draft.id]), buildAccount(draft));
      if (!ok) {
        wizardError = editor.errors[joinPath(['accounts', draft.id])] ?? 'Could not save the account';
        return;
      }
      toasts.info(`Account ${draft.id} saved`);
      if (andTest) {
        step = 3;
        wizardResult = await ipc.accountTest({ account_id: draft.id });
      } else {
        wizardOpen = false;
      }
    } catch (err) {
      wizardError = toIpcError('account_test', err).message;
    } finally {
      saving = false;
    }
  }

  async function remove(id: string): Promise<void> {
    await editor.reset(joinPath(['accounts', id]));
  }

  const accountsNode = $derived(editor.schema ? nodeAt(editor.schema, ['accounts']) : null);
  const itemSchema = $derived(accountsNode ? entrySchema(accountsNode) : null);
  const statusTone = (s: string): 'ok' | 'warn' | 'danger' | 'neutral' =>
    s === 'ok'
      ? 'ok'
      : s === 'needs_auth' || s === 'error'
        ? 'danger'
        : s === 'rate_limited' || s === 'offline'
          ? 'warn'
          : 'neutral';
</script>

<section class="accounts" data-testid="accounts-section">
  <p class="blurb">
    Accounts connect trackers and code hosts. Tokens never live in config files: an account stores a reference
    (<code>keyring:</code>, <code>gh-cli</code>, <code>glab-cli</code>, <code>command:</code> or
    <code>env:</code>).
  </p>

  {#if globalOnly}
    <div class="notice" role="status" data-testid="accounts-global-only">
      <Icon name="info" size={16} />
      <span>Accounts are global settings.</span>
      <Button size="sm" onclick={() => editor.target({ layer: 'global', repoId: null })}
        >Switch to Global</Button
      >
    </div>
  {/if}

  <h3>Secret storage</h3>
  {#if backendError}<p class="error" role="alert">{backendError}</p>{/if}
  <ul class="backends" data-testid="backend-status">
    {#each backends as b (b.backend)}
      {@const advice = !b.available ? backendAdvice(b.backend) : null}
      <li data-backend={b.backend} data-available={b.available}>
        <Icon name={b.available ? 'circle-check' : 'triangle-alert'} size={14} />
        <strong>{b.backend}</strong>
        <span class="muted">{b.detail ?? (b.available ? 'available' : 'unavailable')}</span>
        {#if b.backend === 'encrypted-file' && !b.available}
          <form
            class="advice unlock"
            data-testid="secret-file-unlock"
            onsubmit={(e) => {
              e.preventDefault();
              void unlock();
            }}
          >
            <TextInput
              bind:value={passphrase}
              type="password"
              autocomplete="off"
              label={creating ? 'New passphrase' : 'Passphrase'}
            />
            {#if creating}
              <TextInput
                bind:value={passphraseAgain}
                type="password"
                autocomplete="off"
                label="Repeat the passphrase"
              />
            {/if}
            <Button size="sm" variant="primary" type="submit" loading={unlocking} disabled={!passphrase}
              >{creating ? 'Create' : 'Unlock'}</Button
            >
            {#if unlockError}<p class="error" role="alert">{unlockError}</p>{/if}
          </form>
        {/if}
        {#if advice}
          <div class="advice" data-testid="backend-advice">
            <p>{advice.title}</p>
            <ul>
              {#each advice.steps as s, i (i)}<li>{s}</li>{/each}
            </ul>
            {#each advice.snippets as sn (sn)}<pre>{sn}</pre>{/each}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
  {#if keyringBackend && !keyringBackend.available}
    <p class="hint" data-testid="keyring-fallback">
      The keyring is unavailable: choose the encrypted file, <code>command:</code> or <code>env:</code> when you
      add an account.
    </p>
  {/if}

  <header class="head">
    <h3>Accounts</h3>
    {#if !globalOnly}<Button
        size="sm"
        icon="plus"
        variant="primary"
        onclick={openWizard}
        data-testid="add-account">Add account</Button
      >{/if}
  </header>

  {#if wizardOpen}
    <div class="wizard" data-testid="account-wizard" data-step={step}>
      <ol class="steps">
        {#each ['Service', 'Connection', 'Token', 'Test'] as label, i (label)}
          <li class:current={step === i} class:done={step > i}>{label}</li>
        {/each}
      </ol>

      {#if step === 0}
        <div class="kinds">
          {#each KINDS as k (k.kind)}
            <button type="button" class="kind" data-kind={k.kind} onclick={() => pickKind(k.kind)}>
              <strong>{k.label}</strong>
              <span>{k.blurb}</span>
            </button>
          {/each}
        </div>
      {:else if step === 1}
        {@const info = kindInfo(draft.kind)}
        <div class="form">
          <TextInput
            label="Server URL{info.baseUrlRequired ? '' : ' (optional)'}"
            bind:value={draft.base_url}
            placeholder={info.baseUrlPlaceholder}
            error={errors.base_url}
            onchange={onUrl}
          />
          {#if draft.kind === 'jira'}
            <Select
              label="Flavor"
              value={draft.flavor}
              options={[
                { value: 'auto', label: 'Detect from the URL' },
                { value: 'cloud', label: 'Jira Cloud' },
                { value: 'dc', label: 'Data Center / Server' },
              ]}
              onchange={(v) => (draft.flavor = v as AccountDraft['flavor'])}
            />
          {/if}
          {#if needsEmail(draft)}
            <TextInput
              label="Account e-mail"
              bind:value={draft.email}
              error={errors.email}
              placeholder="me@acme.com"
            />
          {/if}
          {#if info.authOptions.length > 1}
            <Select
              label="Authentication"
              value={draft.auth}
              options={[
                { value: '', label: 'Default for this service' },
                ...info.authOptions.map((a) => ({ value: a, label: a })),
              ]}
              onchange={(v) => (draft.auth = v as AccountDraft['auth'])}
            />
          {/if}
          {#if draft.kind === 'redmine'}
            <Select
              label="Text format"
              value={draft.text_format}
              options={[
                { value: 'textile', label: 'Textile' },
                { value: 'markdown', label: 'Markdown' },
              ]}
              onchange={(v) => (draft.text_format = v as AccountDraft['text_format'])}
            />
          {/if}
          <TextInput
            label="Account id"
            bind:value={draft.id}
            error={errors.id}
            hint="Used in project files and secret names: lowercase letters, digits, dashes."
            oninput={() => (idTouched = true)}
          />
        </div>
        <div class="nav">
          <Button onclick={() => (step = 0)}>Back</Button>
          <Button variant="primary" onclick={toSecret}>Next</Button>
        </div>
      {:else if step === 2}
        <div class="form">
          <p class="muted">Where does Kelta get the token for <strong>{draft.id}</strong>?</p>
          <SecretControl
            value={draft.secret}
            accountKind={draft.kind}
            onchange={(ref) => (draft.secret = ref ?? '')}
            accountId={null}
          />
          {#if errors.secret}<p class="error" role="alert">{errors.secret}</p>{/if}
          {#if wizardError}<p class="error" role="alert" data-testid="wizard-error">{wizardError}</p>{/if}
        </div>
        <div class="nav">
          <Button onclick={() => (step = 1)}>Back</Button>
          <Button onclick={() => save(false)} loading={saving}>Save</Button>
          <Button variant="primary" onclick={() => save(true)} loading={saving} data-testid="save-and-test"
            >Save &amp; test</Button
          >
        </div>
      {:else}
        <div class="form" data-testid="wizard-result">
          {#if wizardResult?.ok}
            <p class="ok">
              <Icon name="circle-check" size={16} /> Connected{wizardResult.user
                ? ` as ${wizardResult.user.name}`
                : ''}.
            </p>
          {:else if wizardResult}
            <p class="error" role="alert">
              <Icon name="circle-x" size={16} />
              {wizardResult.error?.message ?? 'Connection failed'}
            </p>
            <p class="muted">The account is saved. Fix the token or URL and test again from the list.</p>
          {:else}
            <p class="muted">Testing…</p>
          {/if}
        </div>
        <div class="nav"><Button variant="primary" onclick={() => (wizardOpen = false)}>Done</Button></div>
      {/if}
    </div>
  {/if}

  {#if accounts.length === 0 && !wizardOpen}
    <EmptyState
      title="No accounts yet"
      icon="key-round"
      body="Add a tracker or code-host account to see tickets and reviews."
    >
      {#snippet actions()}
        {#if !globalOnly}<Button variant="primary" icon="plus" onclick={openWizard}>Add account</Button>{/if}
      {/snippet}
    </EmptyState>
  {/if}

  {#each accounts as a (a.id)}
    {@const st = settingsStore.accounts[a.id]}
    {@const r = results[a.id]}
    <article class="account" data-testid="account" data-account={a.id}>
      <header>
        <strong>{a.id}</strong>
        <Badge>{kindInfo(a.kind).label}</Badge>
        {#if st}<Badge tone={statusTone(st.status)} title={st.detail ?? undefined}
            >{st.status.replace('_', ' ')}</Badge
          >{/if}
        <span class="muted">{a.base_url ?? ''}</span>
        <span class="spacer"></span>
        <Button size="sm" loading={r === 'running'} onclick={() => test(a.id)}>Test</Button>
        {#if !globalOnly}<IconButton
            icon="trash-2"
            size="sm"
            label={`Remove ${a.id}`}
            onclick={() => remove(a.id)}
          />{/if}
      </header>
      {#if r && r !== 'running'}
        <p class:ok={r.ok} class:error={!r.ok} role="status" data-testid="test-result">
          {r.ok
            ? `Connected${r.user ? ` as ${r.user.name}` : ''}.`
            : (r.error?.message ?? 'Connection failed')}
        </p>
      {/if}
      <details>
        <summary>Settings</summary>
        {#if itemSchema}
          {#each childEntries(itemSchema) as [prop, child] (prop)}
            <Field path={joinPath(['accounts', a.id, prop])} node={child} depth={2} />
          {/each}
        {/if}
      </details>
    </article>
  {/each}
</section>

<style>
  .accounts {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  h3 {
    margin: var(--k-space-4) 0 0;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .blurb,
  .hint,
  .muted {
    margin: 0;
    color: var(--k-fg-muted);
  }

  .notice {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .backends {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  .backends > li {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: var(--k-space-3);
    align-items: baseline;
  }

  .advice {
    grid-column: 1 / -1;
    padding: var(--k-space-3) var(--k-space-4);
    background: var(--k-bg-sunken);
    border-radius: var(--k-radius);
  }

  .unlock {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--k-space-3);
  }

  .advice p {
    margin: 0 0 var(--k-space-2);
    font-weight: 600;
  }

  .advice ul {
    margin: 0 0 var(--k-space-2);
    padding-left: var(--k-space-5);
  }

  .advice pre {
    margin: var(--k-space-1) 0;
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
  }

  .wizard,
  .account {
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    padding: var(--k-space-4);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .steps {
    display: flex;
    gap: var(--k-space-4);
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--k-fg-subtle);
  }

  .steps .current {
    color: var(--k-fg);
    font-weight: 600;
  }

  .steps .done {
    color: var(--k-ok);
  }

  .kinds {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: var(--k-space-3);
  }

  .kind {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: var(--k-space-4);
    text-align: left;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg-elev);
    color: var(--k-fg);
    cursor: pointer;
  }

  .kind:hover {
    border-color: var(--k-accent);
  }

  .kind span {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
    max-width: 520px;
  }

  .nav {
    display: flex;
    gap: var(--k-space-3);
  }

  .account header {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .spacer {
    flex: 1;
  }

  .ok {
    color: var(--k-ok);
    margin: 0;
  }

  .error {
    color: var(--k-danger);
    margin: 0;
  }

  summary {
    cursor: pointer;
    color: var(--k-fg-muted);
  }
</style>
