<script lang="ts">
  // Raw TOML editor for one layer file with live validation (positions from Rust) and an
  // "Open in editor" shortcut. Saving goes through `settings_write_raw`, which validates first.
  import { untrack } from 'svelte';

  import type { ValidationIssue } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';

  import type { SettingsEditor } from './lib/editor.svelte';

  interface Props {
    editor: SettingsEditor;
  }

  let { editor }: Props = $props();

  let text = $state('');
  let loadedText = $state('');
  let issues = $state<ValidationIssue[]>([]);
  let saveError = $state<string | null>(null);
  let saving = $state(false);
  let area = $state<HTMLTextAreaElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  const dirty = $derived(text !== loadedText);
  const doc = $derived(editor.doc);

  let externalText = $state<string | null>(null);

  $effect(() => {
    const t = doc?.text ?? '';
    untrack(() => {
      if (text === loadedText) {
        // no pending edit: follow the file (external change, other layer)
        text = t;
        loadedText = t;
        saveError = null;
        externalText = null;
        void validate(t);
      } else if (t !== loadedText) {
        externalText = t;
      }
    });
  });

  function reloadFromDisk(): void {
    const t = externalText ?? doc?.text ?? '';
    text = t;
    loadedText = t;
    externalText = null;
    void validate(t);
  }

  async function validate(t: string): Promise<void> {
    try {
      issues = await ipc.settingsValidate({ layer: editor.layer, text: t });
    } catch (err) {
      issues = [{ path: '', message: toIpcError('settings_validate', err).message, line: null, col: null }];
    }
  }

  function oninput(): void {
    saveError = null;
    clearTimeout(timer);
    // one-shot: validate 300 ms after the last keystroke
    timer = setTimeout(() => void validate(text), 300);
  }

  async function save(): Promise<void> {
    saving = true;
    saveError = null;
    try {
      await editor.writeRaw(text);
      loadedText = text;
      toasts.info('Saved');
    } catch (err) {
      saveError = toIpcError('settings_write_raw', err).message;
    } finally {
      saving = false;
    }
  }

  async function openInEditor(): Promise<void> {
    try {
      await ipc.settingsOpenFile({
        layer: editor.layer,
        project_id: editor.layer === 'global' ? null : editor.projectId,
        repo_id: editor.layer === 'repo' ? editor.repoId : null,
      });
    } catch (err) {
      toasts.error(err, 'Could not open the file');
    }
  }

  function goTo(i: ValidationIssue): void {
    if (!area || !i.line) return;
    const lines = text.split('\n');
    let offset = 0;
    for (let n = 0; n < i.line - 1 && n < lines.length; n++) offset += lines[n]!.length + 1;
    offset += Math.max(0, (i.col ?? 1) - 1);
    area.focus();
    area.setSelectionRange(offset, offset);
  }
</script>

<section class="edit" data-testid="edit-toml">
  <header>
    <code class="path">{doc?.path || 'no file yet'}</code>
    <span class="spacer"></span>
    <Button size="sm" icon="external-link" onclick={openInEditor}>Open in editor</Button>
    <Button size="sm" onclick={reloadFromDisk} disabled={!dirty}>Revert</Button>
    <Button
      size="sm"
      variant="primary"
      loading={saving}
      disabled={!dirty || issues.length > 0}
      onclick={save}
    >
      Save
    </Button>
  </header>
  {#if externalText !== null}
    <p class="error" role="status" data-testid="toml-external">
      The file changed on disk. <button type="button" class="link" onclick={reloadFromDisk}>Reload it</button> (your
      edit is discarded).
    </p>
  {/if}
  <textarea
    bind:this={area}
    bind:value={text}
    {oninput}
    spellcheck="false"
    aria-label="TOML source"
    aria-invalid={issues.length > 0 || undefined}></textarea>
  {#if issues.length > 0}
    <ul class="issues" data-testid="toml-issues">
      {#each issues as i, n (n)}
        <li>
          <button type="button" onclick={() => goTo(i)}>
            {#if i.line}<span class="loc">{i.line}:{i.col ?? 1}</span>{/if}
            {#if i.path}<code>{i.path}</code>{/if}
            {i.message}
          </button>
        </li>
      {/each}
    </ul>
  {:else if text.trim() !== ''}
    <p class="ok" data-testid="toml-valid">Valid.</p>
  {/if}
  {#if saveError}<p class="error" role="alert">{saveError}</p>{/if}
</section>

<style>
  .edit {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    height: 100%;
    min-height: 320px;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .path {
    color: var(--k-fg-muted);
    word-break: break-all;
  }

  .spacer {
    flex: 1;
  }

  textarea {
    flex: 1;
    min-height: 260px;
    padding: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    resize: none;
    tab-size: 2;
  }

  textarea[aria-invalid='true'] {
    border-color: var(--k-danger);
  }

  .issues {
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--k-danger);
  }

  .issues button {
    border: none;
    background: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    padding: var(--k-space-1) 0;
  }

  .loc {
    font-family: var(--k-font-mono);
    margin-right: var(--k-space-2);
  }

  .link {
    border: none;
    background: none;
    color: var(--k-accent);
    text-decoration: underline;
    cursor: pointer;
    padding: 0;
  }

  .ok {
    margin: 0;
    color: var(--k-ok);
  }

  .error {
    margin: 0;
    color: var(--k-danger);
    white-space: pre-wrap;
  }
</style>
