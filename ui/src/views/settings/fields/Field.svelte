<script lang="ts">
  // One settings key, bound to a dotted path in the edited layer: label, source badge, restart /
  // exec markers, reset-at-this-layer, inline errors from Rust and the type-specific control, laid out
  // by SettingsRow (a group by SettingsGroup; its advanced children render in the section's block).
  import { t } from '$lib/i18n/index.svelte';
  import { settings as store } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  import { groupRuns } from '../copy';
  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SettingsRow from '../layout/SettingsRow.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import { joinPath, splitPath } from '../lib/paths';
  import {
    childEntries,
    fieldKind,
    helpOf,
    infoAt,
    isTemplatePath,
    labelOf,
    repoRelevant,
    resolve,
    TOGGLE_VALUES,
    type SchemaNode,
  } from '../lib/schema';
  import { isAdvanced } from '../lib/sections';
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
    /** Shown but not editable: another setting turns it off. */
    disabled?: boolean;
  }

  let { path, node, depth = 0, title, disabled = false }: Props = $props();

  const editor = useEditor();
  const kind = $derived(TOGGLE_VALUES[path] ? 'bool' : fieldKind(node));
  const segs = $derived(splitPath(path));
  const label = $derived(title ?? labelOf(path));
  const description = $derived(helpOf(path, node));
  const children = $derived(
    kind === 'group'
      ? childEntries(node, path)
          .map(([key, child]): [string, SchemaNode] => [joinPath([...segs, key]), child])
          .filter(([p]) => !isAdvanced(p))
      : [],
  );
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
    <SettingsGroup heading={label} {description} nested={depth > 0} data-testid="group" data-path={path}>
      {#each groupRuns(children) as run, i (i)}
        {#if run.group}
          <SettingsGroup heading={t(run.group)} nested data-testid="copy-group">
            {#each run.items as [p, child] (p)}<Field path={p} node={child} depth={depth + 2} />{/each}
          </SettingsGroup>
        {:else}
          {#each run.items as [p, child] (p)}<Field path={p} node={child} depth={depth + 1} />{/each}
        {/if}
      {/each}
    </SettingsGroup>
  {:else if kind === 'object-map'}
    <ObjectMapField {path} {node} {label} {description} />
  {:else if kind === 'keyed-list'}
    <KeyedListField {path} {node} {label} {description} />
  {:else}
    <SettingsRow
      label={kind !== 'bool' ? label : undefined}
      for={id}
      readonly={!access.editable || disabled}
      data-testid="field"
      data-path={path}
      data-kind={kind}
      data-source={source}
    >
      {#snippet markers()}
        <SourceBadge {source} {path} />
        {#if info?.exec}<Badge
            tone="warn"
            title={t('Runs commands or steers Claude: inert in a repo-local file until trusted')}
            >{t('needs trust')}</Badge
          >{/if}
        {#if restart}<Badge tone="info" title={t('Takes effect after restarting Kelta')}>{t('restart')}</Badge
          >{/if}
        {#if restartPending}<Badge tone="warn" title={t('Changed: restart Kelta to apply')}
            >{t('restart needed')}</Badge
          >{/if}
        {#if setHere && access.editable}
          <IconButton
            icon="rotate-ccw-clock"
            size="sm"
            label={t('Reset {name} at this layer', { name: label })}
            data-testid="reset"
            onclick={() => editor.reset(path)}
          />
        {/if}
      {/snippet}
      {#snippet control()}
        <Control
          {node}
          {path}
          {value}
          {id}
          {label}
          template={isTemplate}
          browse={path === 'plugins.dev_paths'}
          readonly={!access.editable || disabled || !!editor.saving[path]}
          onchange={(v) => void editor.set(path, v)}
        />
      {/snippet}
      {#if description}<p>{description}</p>{/if}
      {#if !access.editable && access.reason}<p>{access.reason}</p>{/if}
      {#if error}<p class="error" role="alert" data-testid="field-error">{error}</p>{/if}
      {#if nullable && value === undefined}<p>{t('Not set.')}</p>{/if}
    </SettingsRow>
  {/if}
{/if}

<style>
  .error {
    color: var(--k-danger);
    white-space: pre-wrap;
  }
</style>
