<script lang="ts">
  // `map<name, object>` (claude.profiles, accounts…): one group per entry whose properties are
  // ordinary path-bound fields, so every leaf keeps its own source badge and reset.
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  import SettingsCard from '../layout/SettingsCard.svelte';
  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import { joinPath, isRecord, splitPath } from '../lib/paths';
  import { childEntries, entrySchema, type SchemaNode } from '../lib/schema';
  import Field from './Field.svelte';
  import SourceBadge from './SourceBadge.svelte';

  interface Props {
    path: string;
    node: SchemaNode;
    label: string;
    description: string;
  }

  let { path, node, label, description }: Props = $props();

  const editor = useEditor();
  const segs = $derived(splitPath(path));
  const item = $derived(entrySchema(node));
  const keys = $derived.by(() => {
    const v = editor.valueOf(path);
    return isRecord(v) ? Object.keys(v) : [];
  });
  const access = $derived(editor.access(path));
  let name = $state('');
  const error = $derived(editor.errors[path] ?? null);

  async function add(): Promise<void> {
    const key = name.trim();
    if (!key || keys.includes(key)) return;
    if (await editor.set(joinPath([...segs, key]), {})) name = '';
  }
</script>

<SettingsGroup heading={label} {description} data-testid="object-map" data-path={path}>
  {#each keys as key (key)}
    {@const entryPath = joinPath([...segs, key])}
    <SettingsCard heading={key} mono data-testid="map-entry" data-path={entryPath}>
      {#snippet head()}
        <SourceBadge source={editor.sourceOf(entryPath)} path={entryPath} />
      {/snippet}
      {#snippet actions()}
        {#if editor.layerValue(entryPath) !== undefined && access.editable}
          <IconButton
            icon="trash-2"
            size="sm"
            label={`Remove ${key} from this layer`}
            onclick={() => editor.reset(entryPath)}
          />
        {/if}
      {/snippet}
      {#if item}
        {#each childEntries(item) as [prop, child] (prop)}
          <Field path={joinPath([...segs, key, prop])} node={child} depth={2} />
        {/each}
      {/if}
    </SettingsCard>
  {/each}
  {#if access.editable}
    <form
      class="add"
      onsubmit={(e) => {
        e.preventDefault();
        void add();
      }}
    >
      <input aria-label={`New ${label} entry name`} placeholder="New entry name" bind:value={name} />
      <Button size="sm" icon="plus" type="submit" disabled={!name.trim()}>Add</Button>
    </form>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</SettingsGroup>

<style>
  .add {
    display: flex;
    gap: var(--k-space-3);
    margin-top: var(--k-space-3);
  }

  .add input {
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-well);
    color: var(--k-fg);
  }

  .error {
    margin: 0;
    color: var(--k-danger);
    font-size: var(--k-font-size-sm);
  }
</style>
