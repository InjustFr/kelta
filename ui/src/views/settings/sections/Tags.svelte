<script lang="ts">
  // Local tags (#349): create, rename / recolour (a rename keeps the tag on its items), delete
  // everywhere after a confirmation. Kept in Kelta's own state, not in a settings layer.
  import { t, tn } from '$lib/i18n/index.svelte';
  import type { SettingsSectionProps } from '$app/registry';
  import { tags } from '$lib/stores';
  import { resolveProjectColor } from '$lib/theme';
  import Button from '$lib/ui/Button.svelte';
  import Dialog from '$lib/ui/Dialog.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';

  import ColorPicker from '../../onboarding/ColorPicker.svelte';
  import SettingsCard from '../layout/SettingsCard.svelte';
  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  /** The tag being edited, by name (a list that shifts under the form cannot retarget it), and the new-tag form. */
  let editing = $state<string | null>(null);
  let adding = $state(false);
  let name = $state('');
  let color = $state<string | null>(null);
  let removing = $state<string | null>(null);

  function edit(tag: { name: string; color: string | null } | null): void {
    editing = tag?.name ?? null;
    adding = tag === null;
    name = tag?.name ?? '';
    color = tag?.color ?? null;
  }

  function close(): void {
    editing = null;
    adding = false;
  }

  async function save(): Promise<void> {
    if ((!adding && editing === null) || !name.trim()) return;
    await tags.put(adding ? null : editing, { name: name.trim(), color });
    close();
  }

  async function confirmRemove(): Promise<void> {
    if (removing !== null) await tags.remove(removing);
    removing = null;
  }
</script>

{#snippet form()}
  <form
    class="edit"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <TextInput label={t('Name')} bind:value={name} />
    <ColorPicker value={color} onchange={(c) => (color = c)} />
    <div class="row">
      <Button type="submit" variant="primary" size="sm" disabled={!name.trim()}>{t('Save')}</Button>
      <Button size="sm" variant="ghost" onclick={close}>{t('Cancel')}</Button>
    </div>
  </form>
{/snippet}

<SectionForm sectionId="tags" {...props}>
  {#snippet before()}
    <SettingsGroup heading={t('Your tags')}>
      {#snippet actions()}
        <Button size="sm" variant="primary" icon="plus" onclick={() => edit(null)}>{t('New tag')}</Button>
      {/snippet}
      {#if adding}
        <SettingsCard data-testid="tag-new">{@render form()}</SettingsCard>
      {/if}
      {#if tags.value.tags.length === 0 && !adding}
        <EmptyState
          icon="tag"
          title={t('No tags yet')}
          body={t('Tags are reminders only you see, on tickets and pull requests.')}
        />
      {/if}
      {#each tags.value.tags as tag (tag.name)}
        <SettingsCard data-testid="tag-row" data-tag={tag.name}>
          {#snippet head()}
            <span class="dot" style:background={resolveProjectColor(tag.color) ?? 'var(--k-border)'}></span>
            <strong>{tag.name}</strong>
            <span class="muted">{tn('{n} item', '{n} items', tags.uses(tag.name))}</span>
          {/snippet}
          {#snippet actions()}
            {#if editing !== tag.name}
              <IconButton
                icon="pencil"
                label={t('Edit {name}', { name: tag.name })}
                size="sm"
                onclick={() => edit(tag)}
              />
              <IconButton
                icon="trash"
                label={t('Delete {name}', { name: tag.name })}
                size="sm"
                onclick={() => (removing = tag.name)}
              />
            {/if}
          {/snippet}
          {#if editing === tag.name}{@render form()}{/if}
        </SettingsCard>
      {/each}
    </SettingsGroup>
  {/snippet}
</SectionForm>

{#if removing !== null}
  <Dialog title={t('Delete tag')} tone="danger" onclose={() => (removing = null)}>
    <p>
      {tn(
        'Delete {name}? It comes off the {n} item that carries it.',
        'Delete {name}? It comes off the {n} items that carry it.',
        tags.uses(removing),
        { name: removing },
      )}
    </p>
    {#snippet actions()}
      <Button variant="ghost" onclick={() => (removing = null)}>{t('Cancel')}</Button>
      <Button variant="danger" onclick={confirmRemove} data-testid="confirm-delete-tag">{t('Delete')}</Button>
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
</style>
