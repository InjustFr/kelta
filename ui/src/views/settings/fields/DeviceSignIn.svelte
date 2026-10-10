<script lang="ts">
  // "Sign in with GitHub / GitLab" (OAuth device flow). The backend keeps the device code and
  // writes the token to `secretRef`; this component only ever sees the user code and the URL.
  import type { AccountKind, OAuthDevicePrompt } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  interface Props {
    kind: AccountKind;
    baseUrl: string;
    /** `keyring:` or `file:` ref that receives the token. */
    secretRef: string;
    /** Called once the token is stored. */
    onsigned: () => void;
  }

  let { kind, baseUrl, secretRef, onsigned }: Props = $props();

  const service = $derived(kind === 'github' ? 'GitHub' : 'GitLab');
  let prompt = $state<OAuthDevicePrompt | null>(null);
  let busy = $state(false);
  let copied = $state(false);
  let error = $state<string | null>(null);
  let gone = false;
  $effect(() => () => {
    gone = true;
    // the wizard closed mid sign-in: stop the backend polling until the code expires
    if (prompt) void ipc.oauthDeviceCancel({ user_code: prompt.user_code }).catch(() => {});
  });

  async function signIn(): Promise<void> {
    busy = true;
    error = null;
    copied = false;
    let cmd = 'oauth_device_start';
    try {
      prompt = await ipc.oauthDeviceStart({ kind, base_url: baseUrl, secret_ref: secretRef });
      cmd = 'oauth_device_finish';
      await ipc.oauthDeviceFinish({ user_code: prompt.user_code });
      if (!gone) onsigned();
    } catch (err) {
      error = toIpcError(cmd, err).message;
    } finally {
      prompt = null;
      busy = false;
    }
  }

  async function copy(code: string): Promise<void> {
    try {
      await ipc.clipboardWrite({ kind: 'clipboard', text: code });
      copied = true;
    } catch (err) {
      error = toIpcError('clipboard_write', err).message;
    }
  }

  const bare = (url: string): string => url.replace(/^https?:\/\//, '');
</script>

<div class="device" data-testid="device-sign-in">
  {#if prompt}
    <p>Enter this code at <strong>{bare(prompt.verification_uri)}</strong> and approve Kelta.</p>
    <div class="code-row">
      <output class="code" data-testid="user-code" aria-label="Sign-in code">{prompt.user_code}</output>
      <IconButton
        icon={copied ? 'check' : 'copy'}
        label={copied ? 'Code copied' : 'Copy code'}
        onclick={() => copy(prompt!.user_code)}
      />
      <Button
        size="sm"
        icon="external-link"
        onclick={() => ipc.openExternal({ url: prompt!.verification_uri }).catch(() => {})}
        >Open {service}</Button
      >
    </div>
    <p class="muted" role="status">
      Waiting for approval. The code works for {Math.max(1, Math.round(prompt.expires_in / 60))} minutes.
    </p>
  {:else}
    <div class="code-row">
      <Button variant="primary" loading={busy} onclick={signIn} data-testid="device-sign-in-start"
        >Sign in with {service}</Button
      >
      <span class="muted">Kelta gets its own token and keeps it in <code>{secretRef}</code>.</span>
    </div>
  {/if}
  {#if error}<p class="error" role="alert" data-testid="device-sign-in-error">{error}</p>{/if}
</div>

<style>
  .device {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  p {
    margin: 0;
  }

  .code-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-3);
  }

  /* The one thing to read across to the browser: large mono, digits and dash evenly spaced. */
  .code {
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-xl);
    line-height: 23px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.08em;
    padding: var(--k-space-1) var(--k-space-3);
    background: var(--k-well);
    border: 1px solid var(--k-border);
    border-radius: 4px;
    user-select: all;
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .error {
    color: var(--k-danger);
  }
</style>
