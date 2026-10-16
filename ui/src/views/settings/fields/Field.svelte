<script lang="ts">
  // One settings key, bound to a dotted path in the edited layer: label, source badge, restart /
  // exec markers, reset-at-this-layer, inline errors from Rust and the type-specific control, laid out
  // by SettingsRow (a group by SettingsGroup; its advanced children render in the section's block).
  import { t } from '$lib/i18n/index.svelte';
  import type { JsonValue } from '$lib/gen';
  import { settings as store } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  import { groupRuns } from '../copy';
  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SettingsRow from '../layout/SettingsRow.svelte';
  import { fileKey, useEditor } from '../lib/editor.svelte';
  import { joinPath, splitPath } from '../lib/paths';
  import {
    childEntries,
    enumOptions,
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
  const globalOnly = $derived(editor.globalOnly(path));
  const inherited = $derived(editor.inherited(path));
  // The inherited value greyed in an empty box: numbers and plain text (a toggle, a menu or a list has no placeholder).
  const hint = $derived(
    inherited != null && !isTemplate && (kind === 'int' || kind === 'float' || kind === 'string'),
  );
  const overrides = $derived(editor.overrides(path));

  /** A value as text: the placeholder, an override chip, a global-only row. */
  function shown(v: JsonValue | undefined): string {
    if (v === undefined || v === null) return t('Not set.');
    const toggle = TOGGLE_VALUES[path];
    if (kind === 'bool') return (toggle ? v === toggle[0] : v === true) ? t('On') : t('Off');
    if (kind === 'enum') return enumOptions(node, path).find((o) => o.value === v)?.label ?? String(v);
    return Array.isArray(v) ? v.join(', ') : typeof v === 'object' ? JSON.stringify(v) : String(v);
  }
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
      label={kind !== 'bool' || globalOnly ? label : undefined}
      for={globalOnly ? undefined : id}
      readonly={(!access.editable && !globalOnly) || disabled}
      actions={globalOnly}
      data-testid="field"
      data-path={path}
      data-kind={globalOnly ? 'global-only' : kind}
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
        {#each overrides as o (fileKey(o.file))}
          <Button
            size="sm"
            variant="ghost"
            data-testid="override"
            title={o.file.path}
            onclick={() => editor.show(o.file, path)}
            >{t('{where} uses {value} →', { where: o.file.where, value: shown(o.value) })}</Button
          >
        {/each}
        {#if setHere && access.editable}
          <IconButton
            icon="rotate-ccw-clock"
            size="sm"
            label={editor.layer === 'project'
              ? t('Use the All projects value again')
              : t('Reset {name} at this layer', { name: label })}
            data-testid="reset"
            onclick={() => editor.reset(path)}
          />
        {/if}
      {/snippet}
      {#snippet control()}
        {#if globalOnly}
          <span data-testid="global-value">{shown(value)}</span>
          <Button
            size="sm"
            variant="ghost"
            onclick={() => editor.show({ layer: 'global', projectId: editor.projectId, repoId: null }, path)}
            >{t('Change in All projects →')}</Button
          >
        {:else}
          <Control
            {node}
            {path}
            value={hint ? undefined : value}
            placeholder={hint ? shown(inherited) : undefined}
            {id}
            {label}
            template={isTemplate}
            browse={path === 'plugins.dev_paths'}
            readonly={!access.editable || disabled || !!editor.saving[path]}
            onchange={(v) => void editor.set(path, v)}
          />
        {/if}
      {/snippet}
      {#if description}<p>{description}</p>{/if}
      {#if globalOnly}<p>{t('The same for every project.')}</p>
      {:else if !access.editable && access.reason}<p>{access.reason}</p>{/if}
      {#if inherited !== undefined}<p data-testid="inherited">{t('Inherited from All projects')}</p>{/if}
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
