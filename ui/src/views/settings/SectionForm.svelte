<script lang="ts">
  // Every section's body: blurb, the section's own content, the top-level schema keys of the section
  // with no per-key code, the rarely changed keys in a collapsed Advanced block, and a link to Edit
  // TOML for the keys no form edits.
  import type { SettingsSectionId, SettingsSectionProps } from '$app/registry';
  import Button from '$lib/ui/Button.svelte';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import type { Snippet } from 'svelte';

  import Field from './fields/Field.svelte';
  import SettingsDisclosure from './layout/SettingsDisclosure.svelte';
  import { useEditor } from './lib/editor.svelte';
  import { joinPath, splitPath } from './lib/paths';
  import { childEntries, fieldKind, nodeAt, repoRelevant, type SchemaNode } from './lib/schema';
  import { ADVANCED, isAdvanced, SECTION_BLURB, SECTION_ROOTS, TOML_ROOTS } from './lib/sections';

  interface Props extends Partial<SettingsSectionProps> {
    sectionId: SettingsSectionId;
    /** Rendered before the generated form. */
    before?: Snippet;
    /** Rendered after the generated form. */
    after?: Snippet;
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
    skip = [],
  }: Props = $props();

  const editor = useEditor(() => ({ layer, projectId, repoId }));
  const roots = $derived((SECTION_ROOTS[sectionId] ?? []).filter((r) => !skip.includes(r)));
  const blurb = $derived(SECTION_BLURB[sectionId]);
  const tomlRoots = $derived(TOML_ROOTS[sectionId] ?? []);

  /** What the form renders: the children of a lone root group, else each root. */
  const entries = $derived.by((): [string, SchemaNode][] => {
    const schema = editor.schema;
    if (!schema) return [];
    const all = SECTION_ROOTS[sectionId] ?? [];
    return roots.flatMap((root): [string, SchemaNode][] => {
      const node = schema.properties?.[root];
      if (!node) return [];
      if (all.length === 1 && fieldKind(node) === 'group')
        return childEntries(node)
          .map(([key, child]): [string, SchemaNode] => [joinPath([root, key]), child])
          .filter(([path]) => !skip.includes(path));
      return [[root, node]];
    });
  });
  const visible = (path: string) => editor.layer !== 'repo' || repoRelevant(path);
  const basic = $derived(entries.filter(([path]) => !isAdvanced(path)));
  const advanced = $derived.by((): [string, SchemaNode][] => {
    const schema = editor.schema;
    if (!schema) return [];
    return ADVANCED.filter(
      (a) => !skip.includes(a) && visible(a) && entries.some(([p]) => a === p || a.startsWith(`${p}.`)),
    ).flatMap((a): [string, SchemaNode][] => {
      const node = nodeAt(schema, splitPath(a));
      return node ? [[a, node]] : [];
    });
  });
</script>

<section class="section" data-testid="settings-section" data-section={sectionId} data-layer={editor.layer}>
  {#if blurb}<p class="blurb">{blurb}</p>{/if}
  {#if before}{@render before()}{/if}
  {#if editor.loadError && !editor.ready}
    <ErrorState title="Settings could not be loaded" error={editor.loadError} onretry={() => editor.load()} />
  {:else if !editor.ready}
    <div class="loading"><Spinner size={16} /> Loading settings…</div>
  {:else}
    {#each basic as [path, node] (path)}
      <Field {path} {node} depth={path.includes('.') ? 1 : 0} />
    {/each}
  {/if}
  {#if after}{@render after()}{/if}
  {#if advanced.length > 0}
    <SettingsDisclosure
      label="Advanced settings"
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
      <span><code>{tomlRoots.join(', ')}</code> {tomlRoots.length === 1 ? 'is' : 'are'} edited as TOML.</span>
      <Button size="sm" variant="ghost" icon="file-code" onclick={() => (editor.raw = true)}>Edit TOML</Button
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
