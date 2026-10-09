<script lang="ts">
  // Generic section body: renders the top-level schema keys of a section with no per-key code.
  import type { SettingsSectionId, SettingsSectionProps } from '$app/registry';
  import ErrorState from '$lib/ui/ErrorState.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import type { Snippet } from 'svelte';

  import Field from './fields/Field.svelte';
  import { useEditor } from './lib/editor.svelte';
  import { SECTION_BLURB, SECTION_ROOTS } from './lib/sections';
  import { childEntries, fieldKind } from './lib/schema';
  import { joinPath } from './lib/paths';

  interface Props extends Partial<SettingsSectionProps> {
    sectionId: SettingsSectionId;
    /** Rendered before the generated form. */
    before?: Snippet;
    /** Rendered after the generated form. */
    after?: Snippet;
  }

  let { sectionId, layer = 'global', projectId = null, repoId = null, before, after }: Props = $props();

  const editor = useEditor(() => ({ layer, projectId, repoId }));
  const roots = $derived(SECTION_ROOTS[sectionId] ?? []);
  const blurb = $derived(SECTION_BLURB[sectionId]);
</script>

<section class="section" data-testid="settings-section" data-section={sectionId} data-layer={editor.layer}>
  {#if blurb}<p class="blurb">{blurb}</p>{/if}
  {#if before}{@render before()}{/if}
  {#if editor.loadError && !editor.ready}
    <ErrorState title="Settings could not be loaded" error={editor.loadError} onretry={() => editor.load()} />
  {:else if !editor.ready}
    <div class="loading"><Spinner size={16} /> Loading settings…</div>
  {:else if editor.schema}
    {@const schema = editor.schema}
    {#each roots as root (root)}
      {@const node = schema.properties?.[root]}
      {#if node}
        {#if roots.length === 1 && fieldKind(node) === 'group'}
          {#each childEntries(node) as [key, child] (key)}
            <Field path={joinPath([root, key])} node={child} depth={1} />
          {/each}
        {:else}
          <Field path={root} {node} />
        {/if}
      {/if}
    {/each}
  {/if}
  {#if after}{@render after()}{/if}
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
  }

  .blurb {
    margin: 0 0 var(--k-space-3);
    color: var(--k-fg-muted);
  }

  .loading {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    padding: var(--k-space-5);
    color: var(--k-fg-muted);
  }
</style>
