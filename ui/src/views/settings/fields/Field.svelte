<script lang="ts">
  // One settings key, bound to a dotted path in the edited layer: label, source badge, restart /
  // exec markers, reset-at-this-layer, inline errors from Rust and the type-specific control, laid out
  // by SettingsRow (a group by SettingsGroup; its advanced children render in the section's block).
  import { t } from '$lib/i18n/index.svelte';
  import { settings as store } from '$lib/stores';
  import Badge from '$lib/ui/Badge.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';

  import SettingsGroup from '../layout/SettingsGroup.svelte';
  import SettingsRow from '../layout/SettingsRow.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import { joinPath, splitPath } from '../lib/paths';
  import {
    childEntries,
    COPY,
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
  }

  let { path, node, depth = 0, title }: Props = $props();

  const editor = useEditor();
  const kind = $derived(fieldKind(node));
  const segs = $derived(splitPath(path));
  const copy = $derived(COPY[path]);
  const label = $derived(title ?? (copy?.label ? t(copy.label) : titleOf(segs[segs.length - 1] ?? path)));
  const description = $derived(copy?.help ? t(copy.help) : descriptionOf(node));
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
      {#each childEntries(node).filter(([key]) => !isAdvanced(joinPath( [...segs, key] ))) as [key, child] (key)}
        <Field path={joinPath([...segs, key])} node={child} depth={depth + 1} />
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
      readonly={!access.editable}
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
            icon="history"
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
          {value}
          {id}
          {label}
          template={isTemplate}
          readonly={!access.editable || !!editor.saving[path]}
          onchange={(v) => void editor.set(path, v)}
        />
      {/snippet}
      {#if description}<p>{description}</p>{/if}
      {#if isTemplate}<p title={t(TEMPLATE_HELP)}>{t('Supports placeholders: hover for the list.')}</p>{/if}
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
