<script lang="ts">
  // `by_id` lists (tools, triggers, session_templates, editor.presets, commands): shows the merged
  // list with the layer each entry comes from, and lets the edited layer override, disable,
  // remove or add entries. Only the edited layer's own list is ever written.
  import type { JsonValue } from '$lib/gen';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  import { useEditor } from '../lib/editor.svelte';
  import { clone, isRecord } from '../lib/paths';
  import { entrySchema, type SchemaNode } from '../lib/schema';
  import Control from './Control.svelte';
  import SourceBadge from './SourceBadge.svelte';

  interface Props {
    path: string;
    node: SchemaNode;
    label: string;
    description: string;
  }

  let { path, node, label, description }: Props = $props();

  type Entry = { [k: string]: JsonValue };

  const editor = useEditor();
  const item = $derived(entrySchema(node));
  const merged = $derived.by((): Entry[] => {
    const v = editor.valueOf(path);
    return Array.isArray(v) ? (v.filter(isRecord) as Entry[]) : [];
  });
  const own = $derived.by((): Entry[] => {
    const v = editor.layerValue(path);
    return Array.isArray(v) ? (v.filter(isRecord) as Entry[]) : [];
  });
  const access = $derived(editor.access(path));
  const error = $derived(editor.errors[path] ?? null);
  let editing = $state<string | null>(null);
  let newId = $state('');

  const idOf = (e: Entry): string => String(e.id ?? '');
  const ownOf = (id: string): Entry | undefined => own.find((e) => idOf(e) === id);

  function sourceOf(id: string): 'default' | 'plugin' | 'global' | 'project' | 'repo' | 'runtime' {
    return editor.sourceOf(`${path}.${id}`);
  }

  async function write(next: Entry[]): Promise<void> {
    if (next.length === 0) await editor.reset(path);
    else await editor.set(path, next as JsonValue);
  }

  async function override(e: Entry): Promise<void> {
    await write([...own.filter((x) => idOf(x) !== idOf(e)), clone(e)]);
  }

  async function setEnabled(e: Entry, enabled: boolean): Promise<void> {
    const mine = ownOf(idOf(e));
    if (mine) {
      const stub = Object.keys(mine).every((k) => k === 'id' || k === 'enabled');
      if (stub && enabled) await write(own.filter((x) => idOf(x) !== idOf(e)));
      else await write(own.map((x) => (idOf(x) === idOf(e) ? { ...x, enabled } : x)));
    } else {
      await write([...own, { id: idOf(e), enabled }]);
    }
  }

  async function removeOwn(id: string): Promise<void> {
    editing = null;
    await write(own.filter((x) => idOf(x) !== id));
  }

  async function replace(id: string, next: JsonValue | null): Promise<void> {
    if (!isRecord(next)) return;
    await write(own.map((x) => (idOf(x) === id ? ({ ...next, id } as Entry) : x)));
  }

  async function add(): Promise<void> {
    const id = newId.trim();
    if (!id || merged.some((e) => idOf(e) === id)) return;
    const props = item?.properties ?? {};
    const fresh: Entry = { id };
    if ('label' in props) fresh.label = id;
    newId = '';
    await write([...own, fresh]);
    editing = id;
  }
</script>

<section class="list" data-testid="keyed-list" data-path={path}>
  <header>
    <h3>{label}</h3>
    {#if description}<p class="desc">{description}</p>{/if}
    <details class="desc">
      <summary>How layering works</summary>
      Entries merge by <code>id</code>: a higher layer replaces the whole entry; a stub with only
      <code>enabled</code> toggles an inherited one.
    </details>
  </header>

  {#each merged as e (idOf(e))}
    {@const id = idOf(e)}
    {@const src = sourceOf(id)}
    {@const mine = ownOf(id)}
    <div class="entry" class:disabled={e.enabled === false} data-testid="list-entry" data-id={id}>
      <div class="entry-head">
        <strong class="id">{id}</strong>
        {#if typeof e.label === 'string' && e.label !== id}<span class="muted">{e.label}</span>{/if}
        <SourceBadge source={src} path={`${path}.${id}`} />
        {#if e.enabled === false}<Badge tone="neutral">disabled</Badge>{/if}
        <span class="actions">
          {#if access.editable}
            {#if src === editor.layer && mine}
              <Button size="sm" onclick={() => (editing = editing === id ? null : id)}
                >{editing === id ? 'Close' : 'Edit'}</Button
              >
              <Button size="sm" onclick={() => setEnabled(e, e.enabled === false)}
                >{e.enabled === false ? 'Enable' : 'Disable'}</Button
              >
              <IconButton
                icon="trash-2"
                size="sm"
                label={`Remove ${id} from this layer`}
                onclick={() => removeOwn(id)}
              />
            {:else}
              <Button size="sm" onclick={() => override(e)}>Override here</Button>
              <Button size="sm" onclick={() => setEnabled(e, e.enabled === false)}
                >{e.enabled === false ? 'Enable' : 'Disable here'}</Button
              >
              {#if mine}<IconButton
                  icon="trash-2"
                  size="sm"
                  label={`Remove ${id} override`}
                  onclick={() => removeOwn(id)}
                />{/if}
            {/if}
          {/if}
        </span>
      </div>
      {#if editing === id && mine && item}
        <Control
          node={item}
          value={mine}
          label={id}
          readonly={!access.editable}
          onchange={(v) => void replace(id, v)}
        />
      {/if}
    </div>
  {:else}
    <p class="desc">No entries.</p>
  {/each}

  {#if access.editable}
    <form
      class="add"
      onsubmit={(e) => {
        e.preventDefault();
        void add();
      }}
    >
      <input aria-label={`New ${label} id`} placeholder="new-id" bind:value={newId} />
      <Button size="sm" icon="plus" type="submit" disabled={!newId.trim()}>Add</Button>
    </form>
  {:else if access.reason}
    <p class="desc">{access.reason}</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .list {
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
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .entry.disabled {
    opacity: 0.65;
  }

  .entry-head {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    flex-wrap: wrap;
  }

  .id {
    font-family: var(--k-font-mono);
  }

  .muted {
    color: var(--k-fg-muted);
  }

  .actions {
    margin-left: auto;
    display: inline-flex;
    gap: var(--k-space-2);
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
    white-space: pre-wrap;
  }
</style>
