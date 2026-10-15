<script lang="ts">
  // A template field's Insert menu and live example (#479): wraps the field's input (`children`, found by
  // `id`), inserts a placeholder at its cursor and shows the core's rendering for the sample ticket KEL-214.
  import { tick, type Snippet } from 'svelte';

  import type { TemplatePreview } from '$lib/gen';
  import { t } from '$lib/i18n/index.svelte';
  import * as ipc from '$lib/ipc/commands';
  import { toIpcError } from '$lib/ipc/transport';
  import Button from '$lib/ui/Button.svelte';
  import Menu, { type MenuItem } from '$lib/ui/Menu.svelte';

  import { PLACEHOLDER_GROUPS } from '../lib/placeholders';

  interface Props {
    /** Id of the wrapped input. */
    id: string;
    /** Settings path: a branch name or worktree folder renders exactly as Start work makes it. */
    path: string;
    /** The field's text; drafts typed into the input are followed too. */
    value: string;
    readonly?: boolean;
    children: Snippet;
  }
  let { id, path, value, readonly = false, children }: Props = $props();

  const input = () => document.getElementById(id) as HTMLInputElement | HTMLTextAreaElement | null;

  // Follows the field's text and the drafts typed into the input.
  let text = $derived(value);
  $effect(() => {
    const el = input();
    if (!el) return;
    const on = () => (text = el.value);
    el.addEventListener('input', on);
    return () => el.removeEventListener('input', on);
  });

  let example = $state<TemplatePreview | { error: string } | null>(null);
  let seq = 0;
  $effect(() => {
    const n = ++seq;
    ipc.templatePreview({ path, template: text }).then(
      (r) => {
        if (n === seq) example = r;
      },
      (e: unknown) => {
        if (n === seq) example = { error: toIpcError('template_preview', e).message };
      },
    );
  });

  const items = $derived<MenuItem[]>(
    PLACEHOLDER_GROUPS.flatMap((g) =>
      g.items.map((p, i) => ({
        id: p.token,
        label: p.token,
        detail: `${t(p.help)} · ${p.example}`,
        group: i === 0 ? t(g.label) : undefined,
      })),
    ),
  );

  let menu = $state<{ x: number; y: number } | null>(null);
  // Where the caret goes back once the save that disabled the input is over (Field: readonly while saving).
  let caret = -1;
  function restore(): void {
    const el = input();
    if (!el || el.disabled || caret < 0) return;
    el.focus();
    el.setSelectionRange(caret, caret);
    caret = -1;
  }
  $effect(() => {
    if (!readonly) restore();
  });

  async function insert(token: string): Promise<void> {
    const el = input();
    if (!el) return;
    const at = el.selectionStart ?? el.value.length;
    el.value = el.value.slice(0, at) + token + el.value.slice(el.selectionEnd ?? at);
    el.dispatchEvent(new Event('input', { bubbles: true })); // the field's draft follows
    el.dispatchEvent(new Event('change', { bubbles: true })); // and commits, as on blur
    caret = at + token.length;
    await tick();
    restore();
  }
</script>

<div class="tpl">
  {@render children()}
  <Button
    size="sm"
    icon="plus"
    disabled={readonly}
    aria-haspopup="menu"
    aria-expanded={!!menu}
    onclick={(e) => {
      const r = e.currentTarget.getBoundingClientRect();
      menu = { x: r.left, y: r.bottom + 4 };
    }}>{t('Insert')}</Button
  >
</div>
{#if example && 'error' in example}
  <p class="example error" data-testid="template-example">
    {t('No example: {message}', { message: example.error })}
  </p>
{:else if example}
  <p class="example" data-testid="template-example">
    {t('Example for KEL-214:')}
    <code>{example.text}</code>
    {#if example.git_ok === true}{t('A valid git branch name.')}{:else if example.git_ok === false}{t(
        'Not a valid git branch name: Kelta uses this cleaned-up name.',
      )}{/if}
  </p>
{/if}
{#if menu}
  <Menu
    {items}
    x={menu.x}
    y={menu.y}
    label={t('Insert a placeholder')}
    onselect={(token) => void insert(token)}
    onclose={() => {
      menu = null;
      input()?.focus();
    }}
  >
    {#snippet footer()}{t(
        'Add |slug, |shell or |json inside the braces to change the format. {a|b} uses b when a is empty.',
      )}{/snippet}
  </Menu>
{/if}

<style>
  .tpl {
    display: flex;
    align-items: flex-end;
    gap: var(--k-space-2);
    min-width: 0;
  }
  .tpl > :global(.k-field) {
    flex: 1;
  }
  .example {
    margin: 0;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .example code {
    font-family: var(--k-font-mono);
    color: var(--k-fg);
  }
  .error {
    color: var(--k-danger);
  }
</style>
