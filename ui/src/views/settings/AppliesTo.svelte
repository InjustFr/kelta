<script lang="ts">
  // "Applies to" (#475): which file the form edits, All projects, one project or one repository; replaces the layer tabs and selects.
  import { t, tn } from '$lib/i18n/index.svelte';
  import { projects } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import Menu, { type MenuItem } from '$lib/ui/Menu.svelte';

  import { fileKey, type EditorTarget, type SettingsEditor } from './lib/editor.svelte';

  interface Props {
    editor: SettingsEditor;
    /** Settings changed in each file, by fileKey. */
    counts: Record<string, number>;
  }
  let { editor, counts }: Props = $props();

  let menu = $state<{ x: number; y: number } | null>(null);
  const choices = $derived.by(() => {
    const own = projects.list.filter((p) => !p.builtin);
    const all: { target: EditorTarget; label: string }[] = [
      { target: { layer: 'global', projectId: null, repoId: null }, label: t('All projects') },
      ...own.map((p) => ({
        target: { layer: 'project' as const, projectId: p.id, repoId: null },
        label: p.name,
      })),
      ...own.flatMap((p) =>
        p.repos.map((r) => ({
          target: { layer: 'repo' as const, projectId: p.id, repoId: r.id },
          label: `${p.name} › ${r.id}`,
        })),
      ),
    ];
    return all.map((c) => ({ ...c, id: fileKey(c.target) }));
  });
  const current = $derived(fileKey(editor));
  const items = $derived(
    choices.map(({ id, target, label }, i): MenuItem => {
      const file = editor.files.find((f) => fileKey(f) === id);
      const n = counts[id] ?? 0;
      const first = choices.findIndex((c) => c.target.layer === target.layer) === i;
      return {
        id,
        label,
        icon: id === current ? 'check' : undefined,
        title: file?.path,
        detail:
          target.layer === 'global'
            ? file?.path
            : target.layer === 'project'
              ? n
                ? tn('{n} change', '{n} changes', n)
                : undefined
              : file?.trusted === false
                ? t('needs trust')
                : undefined,
        group:
          !first || target.layer === 'global'
            ? undefined
            : target.layer === 'project'
              ? t('One project')
              : t(
                  'One repository: its .kelta/config.toml sets tools, triggers, commands, env, worktree and prompt settings only',
                ),
      };
    }),
  );

  function pick(id: string): void {
    const c = choices.find((x) => x.id === id);
    // All projects keeps the project, as the Global tab did (sections still receive it)
    if (c) void editor.target(c.target.layer === 'global' ? { layer: 'global', repoId: null } : c.target);
  }
</script>

<span class="applies">
  <span class="caption">{t('Applies to')}</span>
  <Button
    size="sm"
    icon="chevron-down"
    aria-haspopup="menu"
    aria-expanded={!!menu}
    data-testid="applies-to"
    onclick={(e) => {
      const r = e.currentTarget.getBoundingClientRect();
      menu = { x: r.left, y: r.bottom + 4 };
    }}>{choices.find((c) => c.id === current)?.label ?? t('All projects')}</Button
  >
</span>
{#if menu}
  <Menu {items} x={menu.x} y={menu.y} label={t('Applies to')} onselect={pick} onclose={() => (menu = null)} />
{/if}

<style>
  .applies {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .caption {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }
</style>
