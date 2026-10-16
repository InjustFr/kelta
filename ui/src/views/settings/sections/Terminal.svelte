<script lang="ts">
  import type { SettingsSectionProps } from '$app/registry';

  import { useEditor } from '../lib/editor.svelte';
  import SectionForm from '../SectionForm.svelte';

  let props: SettingsSectionProps = $props();

  const editor = useEditor(() => ({ layer: props.layer, projectId: props.projectId, repoId: props.repoId }));
  const v = (key: string) => editor.valueOf(`terminal.${key}`);
  // The disk caps only matter while scrolled-off lines are saved: shown disabled, not skipped, so search hits still land.
  const disabled = $derived(
    v('history_log') === false ? ['terminal.history_log_mb', 'terminal.history_log_total_mb'] : [],
  );
</script>

<SectionForm sectionId="terminal" {...props} {disabled}>
  {#snippet before()}
    {#if editor.ready}
      <p
        class="preview"
        data-testid="terminal-preview"
        aria-hidden="true"
        style:font-family={String(v('font_family'))}
        style:font-size="{v('font_size')}px"
        style:line-height={String(v('line_height'))}
        style:letter-spacing="{v('letter_spacing')}px"
      >
        $ git commit -m "fix: rank labels first" 0O 1lI {'{}[]'} →
      </p>
    {/if}
  {/snippet}
</SectionForm>

<style>
  .preview {
    margin: 0 0 var(--k-space-2);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg-sunken);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Lines kept per kind of session: one compact grid, not six full-width rows. Three attributes outrank SettingsGroup's own `.body` rule. */
  :global([data-testid='group'][data-path='terminal.scrollback'] > .body) {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    column-gap: var(--k-space-6);
  }
</style>
