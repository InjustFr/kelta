<script lang="ts">
  // Projects: create from a folder, rename / recolour, remove (the config file moves to
  // projects/.trash/), the sources feeding each, plus the environment variables of the layer being edited.
  import { t, tn } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';
  import { dispatch } from '$lib/actions';
  import type { ProjectInfo } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { projects, settings, toasts, ui } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import Button from '$lib/ui/Button.svelte';
  import Dialog from '$lib/ui/Dialog.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';

  import ColorPicker from '../../onboarding/ColorPicker.svelte';
  import SettingsCard from '../layout/SettingsCard.svelte';
  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  const fedBy = (id: string) => (settings.value()?.sources ?? []).filter((s) => s.projects.includes(id));

  const editable = $derived(projects.list.filter((p) => !p.builtin));
  let editing = $state<string | null>(null);
  let name = $state('');
  let color = $state<string | null>(null);
  let icon = $state('');
  let saving = $state(false);
  let removing = $state<ProjectInfo | null>(null);
  let killSessions = $state(false);
  // the file's own `project.color` warning: a bad colour is dropped on load, so `p.color` cannot tell
  let colorWarnings = $state<Record<string, string>>({});

  async function colorWarning(id: string): Promise<string> {
    try {
      const doc = await ipc.settingsLayerGet({ layer: 'project', project_id: id, repo_id: null });
      const issues = await ipc.settingsValidate({ layer: 'project', text: doc.text });
      return issues.find((i) => i.severity === 'warning' && i.path === 'project.color')?.message ?? '';
    } catch {
      return '';
    }
  }

  $effect(() => {
    for (const p of editable) void colorWarning(p.id).then((m) => (colorWarnings[p.id] = m));
  });

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
      toasts.error(err, t('Could not update the project'));
    } finally {
      saving = false;
    }
  }

  async function confirmRemove(): Promise<void> {
    const p = removing;
    if (!p) return;
    try {
      await projects.remove(p.id, killSessions);
      toasts.info(t('{name} moved to projects/.trash', { name: p.name }));
    } catch (err) {
      toasts.error(err, t('Could not remove the project'));
    } finally {
      removing = null;
      killSessions = false;
    }
  }
</script>

<SectionForm sectionId="projects" {...props}>
  {#snippet before()}
    <SettingsGroup heading={t('Your projects')}>
      {#snippet actions()}
        <Button size="sm" variant="primary" icon="folder-plus" onclick={() => ui.openSheet('project_new')}
          >{t('Create project from folder')}</Button
        >
      {/snippet}
      {#if editable.length === 0}
        <EmptyState
          icon="folder-plus"
          title={t('No projects yet')}
          body={t(
            'Only the Home project exists. Create a project from a folder to get tickets, reviews and worktrees.',
          )}
        />
      {:else}
        <div data-testid="project-list">
          {#each editable as p (p.id)}
            <SettingsCard data-testid="project-row" data-project-id={p.id}>
              {#snippet head()}
                <span class="dot" style:background={resolveProjectColor(p.color) ?? 'var(--k-border)'}></span>
                <strong>{p.name}</strong>
                <span class="muted"
                  ><code>{p.id}</code>&ensp;{tn('{n} repo', '{n} repos', p.repos.length)}</span
                >
              {/snippet}
              {#snippet actions()}
                {#if editing !== p.id}
                  <IconButton
                    icon="pencil"
                    label={t('Edit {name}', { name: p.name })}
                    size="sm"
                    onclick={() => edit(p)}
                  />
                  <IconButton
                    icon="trash-2"
                    label={t('Remove {name}', { name: p.name })}
                    size="sm"
                    onclick={() => (removing = p)}
                  />
                {/if}
              {/snippet}
              {#if editing === p.id}
                <form
                  class="edit"
                  onsubmit={(e) => {
                    e.preventDefault();
                    void save(p);
                  }}
                >
                  <TextInput label={t('Name')} bind:value={name} />
                  <div class="row">
                    <ColorPicker value={color} onchange={(c) => (color = c)} />
                    <input
                      class="icon"
                      aria-label={t('Icon')}
                      maxlength="2"
                      placeholder={t('Icon')}
                      bind:value={icon}
                    />
                  </div>
                  {#if colorWarnings[p.id]}
                    <p class="warn" role="status" data-testid="color-warning">
                      {t('Colour ignored: {reason}. Pick one to replace it.', {
                        reason: colorWarnings[p.id] ?? '',
                      })}
                    </p>
                  {/if}
                  <div class="row">
                    <Button type="submit" variant="primary" size="sm" loading={saving}>{t('Save')}</Button>
                    <Button size="sm" variant="ghost" onclick={() => (editing = null)}>{t('Cancel')}</Button>
                  </div>
                </form>
              {:else}
                <span class="muted paths">{p.repos.map((r) => r.path).join(', ')}</span>
                {#if colorWarnings[p.id]}
                  <span class="warn" data-testid="color-warning"
                    >{t('Colour ignored: {reason}', { reason: colorWarnings[p.id] ?? '' })}</span
                  >
                {/if}
                {#if fedBy(p.id).length > 0}
                  <span class="muted" data-testid="fed-by"
                    >{t('Fed by')}
                    {#each fedBy(p.id) as s, i (s.id)}{i > 0 ? ', ' : ''}<button
                        type="button"
                        class="k-link"
                        onclick={() =>
                          void dispatch('settings.open', { section: 'sources', source_id: s.id })}
                        >{s.label}</button
                      >{/each}</span
                  >
                {/if}
              {/if}
            </SettingsCard>
          {/each}
        </div>
      {/if}
    </SettingsGroup>
  {/snippet}
</SectionForm>

{#if removing}
  <Dialog title={t('Remove project')} tone="danger" onclose={() => (removing = null)}>
    <p>
      {t(
        'Remove {name}? Its configuration file moves to projects/.trash/ and can be restored by hand. Repositories and worktrees on disk are not touched.',
        { name: removing.name },
      )}
    </p>
    <Toggle bind:checked={killSessions} label={t('Also stop its running sessions')} />
    {#snippet actions()}
      <Button variant="ghost" onclick={() => (removing = null)}>{t('Cancel')}</Button>
      <Button variant="danger" onclick={confirmRemove} data-testid="confirm-remove">{t('Remove')}</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex: none;
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

  .warn {
    margin: 0;
    color: var(--k-warn);
    font-size: var(--k-font-size-sm);
  }
</style>
