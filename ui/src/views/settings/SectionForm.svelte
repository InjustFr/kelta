<script lang="ts">
  // Every section's body: blurb, the section's own content, the top-level schema keys of the section
  // with no per-key code, the rarely changed keys in a collapsed Advanced block, and a link to Edit
  // TOML for the keys no form edits.
  import { t, tn } from '$lib/i18n/index.svelte';
  import type { SettingsSectionId, SettingsSectionProps } from '$app/registry';
  import Button from '$lib/ui/Button.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import type { Snippet } from 'svelte';

  import { COPY, COPY_DONE, groupRuns, onThisPlatform } from './copy';
  import Field from './fields/Field.svelte';
  import SettingsGroup from './layout/SettingsGroup.svelte';
  import SettingsDisclosure from './layout/SettingsDisclosure.svelte';
  import { useEditor } from './lib/editor.svelte';
  import { joinPath, splitPath } from './lib/paths';
  import { childEntries, fieldKind, leafFields, nodeAt, repoRelevant, type SchemaNode } from './lib/schema';
  import { ADVANCED, SECTION_BLURB, SECTION_ROOTS, TOML_ROOTS, underAdvanced } from './lib/sections';

  interface Props extends Partial<SettingsSectionProps> {
    sectionId: SettingsSectionId;
    /** Rendered before the generated form. */
    before?: Snippet;
    /** Rendered after the generated form. */
    after?: Snippet;
    /** Rendered at the end of each copy group, given its heading (the msg id), for a section's own rows. */
    inGroup?: Snippet<[string]>;
    /** Paths (or whole roots) left out of the generated form (owned by a custom control). */
    skip?: readonly string[];
  }

  let {
    sectionId,
    layer = 'global',
    projectId = null,
    repoId = null,
    before,
    after,
    inGroup,
    skip = [],
  }: Props = $props();

  const editor = useEditor(() => ({ layer, projectId, repoId }));
  const roots = $derived(
    (SECTION_ROOTS[sectionId] ?? []).filter((r) => !skip.includes(r) && onThisPlatform(COPY[r])),
  );
  const blurb = $derived(t(SECTION_BLURB[sectionId] ?? ''));
  const tomlRoots = $derived(TOML_ROOTS[sectionId] ?? []);

  /** What the form renders: the children of a lone root group, the leaves of a finished multi-root section, else each root. */
  const entries = $derived.by((): [string, SchemaNode][] => {
    const schema = editor.schema;
    if (!schema) return [];
    const all = SECTION_ROOTS[sectionId] ?? [];
    // A finished section (every key labelled, COPY_DONE) with several roots: its copy groups span roots,
    // so it shows leaves in the copy order rather than one group per root.
    if (all.length > 1 && COPY_DONE.includes(sectionId))
      return leafFields(schema, roots)
        .map((f): [string, SchemaNode] => [f.path, f.node])
        .filter(([path]) => !skip.includes(path));
    return roots.flatMap((root): [string, SchemaNode][] => {
      const node = schema.properties?.[root];
      if (!node) return [];
      if (all.length === 1 && fieldKind(node) === 'group')
        return childEntries(node, root)
          .map(([key, child]): [string, SchemaNode] => [joinPath([root, key]), child])
          .filter(([path]) => !skip.includes(path));
      return [[root, node]];
    });
  });
  const visible = (path: string) => editor.layer !== 'repo' || repoRelevant(path);
  const basic = $derived(entries.filter(([path]) => !underAdvanced(path)));
  const advanced = $derived.by((): [string, SchemaNode][] => {
    const schema = editor.schema;
    if (!schema) return [];
    return ADVANCED.filter(
      (a) =>
        !skip.includes(a) &&
        visible(a) &&
        onThisPlatform(COPY[a]) &&
        entries.some(([p]) => a === p || a.startsWith(`${p}.`) || p.startsWith(`${a}.`)),
    ).flatMap((a): [string, SchemaNode][] => {
      const node = nodeAt(schema, splitPath(a));
      return node ? [[a, node]] : [];
    });
  });
</script>

{#snippet fields(items: [string, SchemaNode][])}
  {#each items as [path, node] (path)}
    <Field {path} {node} depth={path.includes('.') ? 1 : 0} />
  {/each}
{/snippet}

<section class="section" data-testid="settings-section" data-section={sectionId} data-layer={editor.layer}>
  {#if blurb}<p class="blurb">{blurb}</p>{/if}
  {#if before}{@render before()}{/if}
  {#if editor.loadError && !editor.ready}
    <ErrorState
      title={t('Settings could not be loaded')}
      error={editor.loadError}
      onretry={() => editor.load()}
    />
  {:else if !editor.ready}
    <div class="loading"><Spinner size={16} /> {t('Loading settings…')}</div>
  {:else}
    {#each groupRuns(basic.filter(([path]) => visible(path))) as run, i (i)}
      {#if run.group}
        <SettingsGroup heading={t(run.group)} data-testid="copy-group" data-group={run.group}
          >{@render fields(run.items)}{@render inGroup?.(run.group)}</SettingsGroup
        >
      {:else}
        {@render fields(run.items)}
      {/if}
    {/each}
  {/if}
  {#if after}{@render after()}{/if}
  {#if advanced.length > 0}
    <SettingsDisclosure
      label={t('Advanced settings')}
      count={advanced.length}
      class="advanced"
      data-testid="advanced"
    >
      {#each advanced as [path, node] (path)}
        <Field {path} {node} depth={1} />
      {/each}
    </SettingsDisclosure>
  {/if}
  {#if tomlRoots.length > 0}
    <p class="toml" data-testid="toml-only">
      <span
        ><code>{tomlRoots.join(', ')}</code>
        {tn('is edited as TOML.', 'are edited as TOML.', tomlRoots.length)}</span
      >
      <Button size="sm" variant="ghost" icon="file-code" onclick={() => (editor.raw = true)}
        >{t('Edit TOML')}</Button
      >
    </p>
  {/if}
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
  }

  .blurb {
    margin: 0 0 var(--k-space-3);
    max-width: var(--k-measure);
    color: var(--k-fg-muted);
  }

  .toml {
    margin: var(--k-space-6) 0 0;
    color: var(--k-fg-muted);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
    font-size: var(--k-font-size-sm);
  }

  .loading {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-5);
    color: var(--k-fg-muted);
  }

  .section > :global(.advanced) {
    margin-top: var(--k-space-6);
  }
</style>
