<script lang="ts">
  // One settings key, bound to a dotted path in the edited layer: label, source badge, restart /
  // exec markers, reset-at-this-layer, inline errors from Rust and the type-specific control.
  import { settings as store } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  import { useEditor } from '../lib/editor.svelte';
  import { joinPath, splitPath } from '../lib/paths';
  import {
    childEntries,
    descriptionOf,
    fieldKind,
    infoAt,
    isTemplatePath,
    repoRelevant,
    resolve,
    TEMPLATE_HELP,
    titleOf,
    type SchemaNode,
  } from '../lib/schema';
  import Control from './Control.svelte';
  import Field from './Field.svelte';
  import KeyedListField from './KeyedListField.svelte';
  import ObjectMapField from './ObjectMapField.svelte';
  import SourceBadge from './SourceBadge.svelte';

  interface Props {
    path: string;
    node: SchemaNode;
    /** Heading level of groups (visual nesting). */
    depth?: number;
    /** Override the derived title. */
    title?: string;
  }

  let { path, node, depth = 0, title }: Props = $props();

  const editor = useEditor();
  const kind = $derived(fieldKind(node));
  const segs = $derived(splitPath(path));
  const label = $derived(title ?? titleOf(segs[segs.length - 1] ?? path));
  const description = $derived(descriptionOf(node));
  const info = $derived(editor.schema ? infoAt(editor.schema, segs) : null);
  const access = $derived(editor.access(path));
  const source = $derived(editor.sourceOf(path));
  const value = $derived(editor.valueOf(path));
  const error = $derived(editor.errors[path] ?? null);
  const setHere = $derived(editor.isSetHere(path));
  const restart = $derived(!!info?.restart);
  const restartPending = $derived(store.pendingRestart.includes(path));
  const id = $derived(`f-${path}`);
  const isTemplate = $derived(isTemplatePath(path));
  const nullable = $derived(resolve(node).nullable);
  const visible = $derived(editor.layer !== 'repo' || repoRelevant(path));
</script>

{#if visible}
  {#if kind === 'group'}
    <section class="group depth-{depth}" data-testid="group" data-path={path}>
      <header>
        {#if depth === 0}<h3>{label}</h3>{:else}<h4>{label}</h4>{/if}
        {#if description}<p class="desc">{description}</p>{/if}
      </header>
      <div class="children">
        {#each childEntries(node) as [key, child] (key)}
          <Field path={joinPath([...segs, key])} node={child} depth={depth + 1} />
        {/each}
      </div>
    </section>
  {:else if kind === 'object-map'}
    <ObjectMapField {path} {node} {label} {description} />
  {:else if kind === 'keyed-list'}
    <KeyedListField {path} {node} {label} {description} />
  {:else}
    <div
      class="field"
      class:readonly={!access.editable}
      data-testid="field"
      data-path={path}
      data-kind={kind}
      data-source={source}
    >
      <div class="head">
        {#if kind !== 'bool'}<label for={id}>{label}</label>{:else}<span class="spacer"></span>{/if}
        <span class="markers">
          <SourceBadge {source} {path} />
          {#if info?.exec}<Badge tone="warn" title="Runs commands: inert in a repo-local file until trusted"
              >runs commands</Badge
            >{/if}
          {#if restart}<Badge tone="info" title="Takes effect after restarting Kelta">restart</Badge>{/if}
          {#if restartPending}<Badge tone="warn" title="Changed: restart Kelta to apply">restart needed</Badge
            >{/if}
          {#if setHere && access.editable}
            <IconButton
              icon="history"
              size="sm"
              label={`Reset ${label} at this layer`}
              data-testid="reset"
              onclick={() => editor.reset(path)}
            />
          {/if}
        </span>
      </div>
      <Control
        {node}
        {value}
        {id}
        {label}
        template={isTemplate}
        readonly={!access.editable || !!editor.saving[path]}
        onchange={(v) => void editor.set(path, v)}
      />
      {#if description && kind !== 'bool'}<p class="desc">{description}</p>{:else if description}<p
          class="desc"
        >
          {description}
        </p>{/if}
      {#if isTemplate}<p class="desc template-help" title={TEMPLATE_HELP}>
          Supports {'{placeholders}'}: hover for the list.
        </p>{/if}
      {#if !access.editable && access.reason}<p class="desc">{access.reason}</p>{/if}
      {#if error}<p class="error" role="alert" data-testid="field-error">{error}</p>{/if}
      {#if nullable && value === undefined}<p class="desc">Not set.</p>{/if}
    </div>
  {/if}
{/if}

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: var(--k-space-3) 0;
    border-bottom: 1px solid var(--k-border);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--k-space-3);
    min-height: 22px;
  }

  label {
    font-weight: 600;
  }

  .markers {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
    margin-left: auto;
  }

  .desc {
    margin: 0;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .error {
    margin: 0;
    color: var(--k-danger);
    font-size: var(--k-font-size-sm);
    white-space: pre-wrap;
  }

  .readonly {
    opacity: 0.7;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin-top: var(--k-space-4);
  }

  .group header h3,
  .group header h4 {
    margin: 0;
  }

  h3 {
    font-size: var(--k-font-size-lg);
  }

  h4 {
    font-size: var(--k-font-size);
    color: var(--k-fg-muted);
  }

  .children {
    display: flex;
    flex-direction: column;
  }

  .depth-1 > .children,
  .depth-2 > .children {
    padding-left: var(--k-space-4);
    border-left: 2px solid var(--k-border);
  }
</style>
