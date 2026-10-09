<script lang="ts">
  // "New project" (SPEC §2): pick a folder, `project_detect` infers repos, remotes and tracker
  // hints, the user edits the draft and `project_create` writes projects/<id>.toml.
  import type { SheetProps } from '$app/registry';
  import type { ProjectDraft } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import { projects, settings, toasts } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Sheet from '$lib/ui/Sheet.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  import {
    PALETTE,
    bindTracker,
    hasIssues,
    newRepo,
    removeRepo,
    setPrimary,
    slugify,
    validateDraft,
  } from './draft';

  let { onclose, path: initialPath }: SheetProps = $props();

  let path = $state('');
  $effect.pre(() => {
    if (typeof initialPath === 'string' && path === '') path = initialPath;
  });
  let draft = $state<ProjectDraft | null>(null);
  let detecting = $state(false);
  let creating = $state(false);
  let error = $state<string | null>(null);
  let idTouched = $state(false);

  const accounts = $derived(Object.entries(settings.value()?.accounts ?? {}));
  const accountOptions = $derived([
    { value: '', label: 'No tracker' },
    ...accounts.map(([id, a]) => ({ value: id, label: `${id} (${a.kind})` })),
  ]);
  const issues = $derived(
    draft
      ? validateDraft(
          draft,
          projects.list.map((p) => p.id),
        )
      : null,
  );

  $effect(() => {
    if (settings.value() === null) void settings.load().catch(() => undefined);
  });

  async function detect(): Promise<void> {
    if (!path.trim()) return;
    detecting = true;
    error = null;
    try {
      draft = await ipc.projectDetect({ path: path.trim() });
      idTouched = false;
    } catch (err) {
      draft = null;
      error = toIpcError('project_detect', err).message;
    } finally {
      detecting = false;
    }
  }

  function onName(): void {
    if (draft && !idTouched) draft.suggested_id = slugify(draft.name);
  }

  function pickTracker(account: string): void {
    if (!draft) return;
    if (!account) {
      draft.tracker = null;
      return;
    }
    const kind = settings.value()?.accounts[account]?.kind ?? 'github';
    const hint = draft.tracker_hints.find((h) => h.account === account || h.kind === kind)?.key ?? null;
    draft.tracker = bindTracker(account, kind, hint);
  }

  async function create(): Promise<void> {
    if (!draft || !issues || hasIssues(issues)) return;
    creating = true;
    error = null;
    try {
      const info = await ipc.projectCreate({ draft: $state.snapshot(draft) as ProjectDraft });
      await projects.activate(info.id).catch(() => undefined);
      toasts.info(`Project ${info.name} created`);
      onclose();
    } catch (err) {
      error = toIpcError('project_create', err).message;
    } finally {
      creating = false;
    }
  }
</script>

<Sheet title="New project" {onclose} width={620}>
  <form
    class="sheet"
    onsubmit={(e) => {
      e.preventDefault();
      if (draft) void create();
      else void detect();
    }}
  >
    <div class="pick">
      <TextInput
        label="Folder"
        bind:value={path}
        placeholder="~/Sites/shop"
        hint="A git repository, or a folder containing several."
        data-testid="project-path"
      />
      <Button icon="folder-open" loading={detecting} disabled={!path.trim()} onclick={detect}>Detect</Button>
    </div>

    {#if error}<p class="error" role="alert" data-testid="project-error">{error}</p>{/if}

    {#if draft && issues}
      <TextInput label="Name" bind:value={draft.name} oninput={onName} error={issues.name} />
      <TextInput
        label="Id"
        bind:value={draft.suggested_id}
        oninput={() => (idTouched = true)}
        error={issues.id}
        hint="Becomes projects/&lt;id&gt;.toml."
      />

      <div class="colors" role="group" aria-label="Colour">
        {#each PALETTE as c (c)}
          <button
            type="button"
            class="swatch"
            class:on={draft.color === c}
            style:background={c}
            aria-label={c}
            aria-pressed={draft.color === c}
            onclick={() => draft && (draft.color = draft.color === c ? null : c)}
          ></button>
        {/each}
        <input
          class="icon"
          aria-label="Icon (1-2 characters)"
          placeholder="Icon"
          maxlength="2"
          value={draft.icon ?? ''}
          oninput={(e) => draft && (draft.icon = e.currentTarget.value || null)}
        />
      </div>

      <fieldset>
        <legend>Repositories</legend>
        {#if issues.repos}<p class="error">{issues.repos}</p>{/if}
        {#each draft.repos as repo, i (i)}
          <div class="repo" data-testid="repo-row">
            <div class="repo-head">
              <label class="radio">
                <input
                  type="radio"
                  name="primary"
                  checked={repo.primary}
                  onchange={() => draft && (draft.repos = setPrimary(draft.repos, i))}
                />
                Primary
              </label>
              {#if repo.code_host}<Badge tone="info">{repo.code_host.repo}</Badge>{/if}
              <span class="spacer"></span>
              <IconButton
                icon="trash-2"
                label="Remove repository"
                onclick={() => draft && (draft.repos = removeRepo(draft.repos, i))}
              />
            </div>
            <div class="grid">
              <TextInput label="Id" bind:value={repo.id} error={issues.repo[i]} />
              <TextInput label="Path" bind:value={repo.path} />
              <TextInput label="Remote" bind:value={repo.remote} />
              <TextInput label="Base branch" bind:value={repo.base} />
            </div>
            {#if draft.code_host_hints.find((h) => h.repo_id === repo.id)}
              {@const hint = draft.code_host_hints.find((h) => h.repo_id === repo.id)!}
              <p class="hint">
                {hint.kind} · {hint.host}/{hint.repo}
                {#if hint.account}· account {hint.account}{:else}· no matching account yet{/if}
              </p>
            {/if}
          </div>
        {/each}
        <Button
          size="sm"
          icon="plus"
          onclick={() => draft && (draft.repos = [...draft.repos, newRepo(draft.repos.length, false)])}
          >Add repository</Button
        >
      </fieldset>

      <fieldset>
        <legend>Tracker</legend>
        {#each draft.tracker_hints as h (h.kind + (h.key ?? ''))}
          <p class="hint">Detected: {h.kind}{h.key ? ` ${h.key}` : ''} — {h.reason}</p>
        {/each}
        <Select
          label="Tracker account"
          value={draft.tracker?.account ?? ''}
          options={accountOptions}
          onchange={pickTracker}
        />
        {#if accounts.length === 0}
          <p class="hint">No accounts yet. Add one in Settings → Accounts, then bind it here later.</p>
        {/if}
      </fieldset>
    {/if}
  </form>

  {#snippet actions()}
    <Button variant="ghost" onclick={onclose}>Cancel</Button>
    <Button
      variant="primary"
      loading={creating}
      disabled={!draft || !issues || hasIssues(issues)}
      onclick={create}
      data-testid="project-create">Create project</Button
    >
  {/snippet}
</Sheet>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
  }

  .pick {
    display: flex;
    align-items: flex-end;
    gap: var(--k-space-3);
  }

  .pick :global(.k-field) {
    flex: 1;
  }

  fieldset {
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    padding: var(--k-space-3) var(--k-space-4);
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  legend {
    padding: 0 var(--k-space-2);
    color: var(--k-fg-muted);
  }

  .repo {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding-bottom: var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
  }

  .repo-head {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .radio {
    display: inline-flex;
    gap: var(--k-space-2);
    align-items: center;
  }

  .spacer {
    flex: 1;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--k-space-3);
  }

  .colors {
    display: flex;
    gap: var(--k-space-2);
    align-items: center;
  }

  .swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
  }

  .swatch.on {
    border-color: var(--k-fg);
  }

  .icon {
    width: 56px;
    margin-left: var(--k-space-3);
  }

  .hint {
    margin: 0;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .error {
    margin: 0;
    color: var(--k-danger);
  }
</style>
