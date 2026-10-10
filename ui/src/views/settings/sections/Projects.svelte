<script lang="ts">
  // Projects: create from a folder, rename / recolour, remove (the config file moves to
  // projects/.trash/), their ticket sources, plus the environment variables of the layer being edited.
  import type { SettingsSectionProps } from '$app/registry';
  import type { ProjectInfo } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, toasts, ui } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import Dialog from '$lib/ui/Dialog.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';

  import ColorPicker from '../../onboarding/ColorPicker.svelte';
  import SectionForm from '../SectionForm.svelte';
  import TrackerSources from '../TrackerSources.svelte';

  let props: SettingsSectionProps = $props();

  const editable = $derived(projects.list.filter((p) => !p.builtin));
  let editing = $state<string | null>(null);
  let name = $state('');
  let color = $state<string | null>(null);
  let icon = $state('');
  let saving = $state(false);
  let removing = $state<ProjectInfo | null>(null);
  let killSessions = $state(false);

  function edit(p: ProjectInfo): void {
    editing = p.id;
    name = p.name;
    color = p.color;
    icon = p.icon ?? '';
  }

  async function save(p: ProjectInfo): Promise<void> {
    saving = true;
    try {
      await ipc.projectUpdate({
        id: p.id,
        patch: {
          name: name.trim() || p.name,
          color,
          icon: icon || null,
          default_template: null,
          repos: null,
          tracker: null,
          remove_tracker: false,
        },
      });
      editing = null;
    } catch (err) {
      toasts.error(err, 'Could not update the project');
    } finally {
      saving = false;
    }
  }

  async function confirmRemove(): Promise<void> {
    const p = removing;
    if (!p) return;
    try {
      await projects.remove(p.id, killSessions);
      toasts.info(`${p.name} moved to projects/.trash`);
    } catch (err) {
      toasts.error(err, 'Could not remove the project');
    } finally {
      removing = null;
      killSessions = false;
    }
  }
</script>

<SectionForm sectionId="projects" {...props}>
  {#snippet before()}
    <div class="head">
      <h3>Your projects</h3>
      <Button size="sm" variant="primary" icon="folder-plus" onclick={() => ui.openSheet('project_new')}
        >Create project from folder</Button
      >
    </div>
    {#if editable.length === 0}
      <EmptyState
        icon="folder-plus"
        title="No projects yet"
        body="Only the Home project exists. Create a project from a folder to get tickets, reviews and worktrees."
      />
    {:else}
      <ul class="list" data-testid="project-list">
        {#each editable as p (p.id)}
          <li data-testid="project-row" data-project-id={p.id}>
            {#if editing === p.id}
              <form
                class="edit"
                onsubmit={(e) => {
                  e.preventDefault();
                  void save(p);
                }}
              >
                <TextInput label="Name" bind:value={name} />
                <div class="row">
                  <ColorPicker value={color} onchange={(c) => (color = c)} />
                  <input class="icon" aria-label="Icon" maxlength="2" placeholder="Icon" bind:value={icon} />
                </div>
                <div class="row">
                  <Button type="submit" variant="primary" size="sm" loading={saving}>Save</Button>
                  <Button size="sm" variant="ghost" onclick={() => (editing = null)}>Cancel</Button>
                </div>
              </form>
            {:else}
              <span class="dot" style:background={p.color ?? 'var(--k-border)'}></span>
              <div class="info">
                <strong>{p.name}</strong>
                <span class="muted"
                  ><code>{p.id}</code>&ensp;{p.repos.length} repo{p.repos.length === 1 ? '' : 's'}</span
                >
                <span class="muted paths">{p.repos.map((r) => r.path).join(', ')}</span>
              </div>
              <IconButton icon="pencil" label="Edit {p.name}" size="sm" onclick={() => edit(p)} />
              <IconButton icon="trash-2" label="Remove {p.name}" size="sm" onclick={() => (removing = p)} />
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    <TrackerSources projectId={props.projectId} />
    <h3 class="env">Environment</h3>
  {/snippet}
</SectionForm>

{#if removing}
  <Dialog title="Remove project" tone="danger" onclose={() => (removing = null)}>
    <p>
      Remove <strong>{removing.name}</strong>? Its configuration file moves to <code>projects/.trash/</code>
      and can be restored by hand. Repositories and worktrees on disk are not touched.
    </p>
    <Toggle bind:checked={killSessions} label="Also stop its running sessions" />
    {#snippet actions()}
      <Button variant="ghost" onclick={() => (removing = null)}>Cancel</Button>
      <Button variant="danger" onclick={confirmRemove} data-testid="confirm-remove">Remove</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--k-space-3);
  }

  h3 {
    margin: 0;
  }

  .env {
    margin: var(--k-space-5) 0 var(--k-space-3);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex: none;
  }

  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .muted {
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
  }

  .paths {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .edit {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .row {
    display: flex;
    gap: var(--k-space-3);
    align-items: center;
  }

  .icon {
    width: 56px;
  }
</style>
