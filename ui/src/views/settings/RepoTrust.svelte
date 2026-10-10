<script lang="ts">
  // Repo-local `.kelta/config.toml`: untrusted until the user reviewed it (trust is per content
  // hash; any edit makes it untrusted again).
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import Dialog from '$lib/ui/Dialog.svelte';
  import Icon from '$lib/ui/Icon.svelte';

  import type { SettingsEditor } from './lib/editor.svelte';

  interface Props {
    editor: SettingsEditor;
  }

  let { editor }: Props = $props();

  let reviewing = $state(false);
  let busy = $state(false);

  const doc = $derived(editor.doc);

  // Trust is bound to the text shown in the dialog: the core refuses it if the file changed since.
  async function sha256(text: string): Promise<string> {
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('');
  }

  async function setTrust(trust: boolean): Promise<void> {
    if (!editor.projectId || !editor.repoId || !doc) return;
    busy = true;
    try {
      const hash = trust ? await sha256(doc.text) : undefined;
      await ipc.repoTrust({ project_id: editor.projectId, repo_id: editor.repoId, trust, sha256: hash });
      reviewing = false;
      await editor.load();
      toasts.info(trust ? 'Repo config trusted' : 'Trust revoked');
    } catch (err) {
      toasts.error(toIpcError('repo_trust', err).message);
    } finally {
      busy = false;
    }
  }
</script>

{#if doc}
  {#if doc.trusted === null}
    <div class="banner info" data-testid="repo-no-file">
      <Icon name="info" size={16} />
      <span>
        This repo has no <code>.kelta/config.toml</code>. Allowed keys: tools, triggers, commands, session
        templates,
        <code>env</code>, worktree include / setup / branch template, Claude system prompt, editor review
        args.
      </span>
    </div>
  {:else if doc.trusted === false}
    <div class="banner warn" data-testid="repo-untrusted">
      <Icon name="triangle-alert" size={16} />
      <span>
        This repo's <code>.kelta/config.toml</code> wants to run commands. Its executable keys (tools,
        triggers, commands, <code>worktree.setup</code>, <code>env</code>) are inert until you review and
        trust it.
      </span>
      <Button size="sm" variant="primary" onclick={() => (reviewing = true)}>Review &amp; trust</Button>
    </div>
  {:else}
    <div class="banner ok" data-testid="repo-trusted">
      <Icon name="shield-check" size={16} />
      <span>This exact content is trusted. Editing it makes it untrusted again.</span>
      <Button size="sm" onclick={() => setTrust(false)} loading={busy}>Revoke trust</Button>
    </div>
  {/if}
{/if}

{#if reviewing && doc}
  <Dialog title="Review repo config" width={640} onclose={() => (reviewing = false)}>
    <p class="path">{doc.path}</p>
    <pre class="file" data-testid="repo-config-text">{doc.text}</pre>
    {#snippet actions()}
      <Button onclick={() => (reviewing = false)}>Cancel</Button>
      <Button variant="primary" loading={busy} onclick={() => setTrust(true)}>Trust this content</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    margin-bottom: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .banner span {
    flex: 1;
  }

  .warn {
    border-color: var(--k-warn);
  }

  .ok {
    border-color: var(--k-ok);
  }

  .path {
    margin: 0 0 var(--k-space-3);
    font-family: var(--k-font-mono);
    color: var(--k-fg-muted);
    word-break: break-all;
  }

  .file {
    margin: 0;
    padding: var(--k-space-3);
    max-height: 40vh;
    overflow: auto;
    background: var(--k-bg-sunken);
    border-radius: var(--k-radius);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
  }
</style>
