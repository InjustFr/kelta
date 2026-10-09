<script lang="ts">
  // `map<name, object>` (claude.profiles, accounts…): one group per entry whose properties are
  // ordinary path-bound fields, so every leaf keeps its own source badge and reset.
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

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

<section class="map" data-testid="object-map" data-path={path}>
  <header>
    <h3>{label}</h3>
    {#if description}<p class="desc">{description}</p>{/if}
  </header>
  {#each keys as key (key)}
    {@const entryPath = joinPath([...segs, key])}
    <div class="entry" data-testid="map-entry" data-path={entryPath}>
      <div class="entry-head">
        <h4>{key}</h4>
        <SourceBadge source={editor.sourceOf(entryPath)} path={entryPath} />
        {#if editor.layerValue(entryPath) !== undefined && access.editable}
          <IconButton
            icon="trash-2"
            size="sm"
            label={`Remove ${key} from this layer`}
            onclick={() => editor.reset(entryPath)}
          />
        {/if}
      </div>
      {#if item}
        {#each childEntries(item) as [prop, child] (prop)}
          <Field path={joinPath([...segs, key, prop])} node={child} depth={2} />
        {/each}
      {/if}
    </div>
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
</section>

<style>
  .map {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    margin-top: var(--k-space-4);
  }

  header h3 {
    margin: 0;
    font-size: var(--k-font-size-lg);
  }

  .desc {
    margin: 0;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .entry {
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .entry-head {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .entry-head h4 {
    margin: 0;
    font-family: var(--k-font-mono);
  }

  .add {
    display: flex;
    gap: var(--k-space-3);
  }

  .add input {
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
  }

  .error {
    margin: 0;
    color: var(--k-danger);
    font-size: var(--k-font-size-sm);
  }
</style>
