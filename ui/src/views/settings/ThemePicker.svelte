<script lang="ts">
  // Settings → General (global layer): one card grid per mode for `app.dark_theme` / `app.light_theme`.
  // Arrows only move focus; Enter, Space or a click writes the slot (no write per focused card).
  // Import theme... converts a VS Code (#231) or terminal (#232) theme file and makes it the theme of its base slot.
  import { tick } from 'svelte';

  import type { JsonValue, ThemeDef } from '$lib/gen';
  import { settings, toasts } from '$lib/stores';
  import { colors, type ThemeMode } from '$lib/terminal/theme';
  import { resolveTheme } from '$lib/theme';
  import { BUILTIN } from '$lib/theme/builtin';
  import { Badge, Button, Icon, IconButton } from '$lib/ui';

  import { confirms } from '../../shell/confirm.svelte';
  import { useEditor } from './lib/editor.svelte';
  import { joinPath } from './lib/paths';

  const editor = useEditor();
  const SLOT = { dark: 'dark_theme', light: 'light_theme' } as const;

  const s = $derived(settings.value());
  const user = $derived(s?.themes ?? {});
  const groups = $derived(
    (['dark', 'light'] as const).map((mode) => {
      const current = s?.app[SLOT[mode]] ?? `bezel-${mode}`;
      const cards = Object.entries({ ...BUILTIN, ...user })
        .filter(([, def]) => def.base === mode)
        .map(([id, def]) => ({ id, def, custom: id in user, preview: preview(id, mode) }));
      return { mode, current, cards, missing: !cards.some((c) => c.id === current) };
    }),
  );
  /** The card that holds tabindex=0 in each grid (roving tabindex). */
  let focused = $state<Record<ThemeMode, string | null>>({ dark: null, light: null });

  function preview(id: string, mode: ThemeMode) {
    const r = resolveTheme(s && { ...s, app: { ...s.app, [SLOT[mode]]: id } }, mode);
    const base = colors(mode);
    return {
      well: r.vars['--k-well'] ?? base.background,
      fg: r.vars['--k-fg'] ?? base.foreground,
      ansi: r.terminal.ansi.slice(1, 7),
      // Bezel's accent follows the project hue, so the live one is its honest preview.
      accent: r.accent ?? 'var(--k-accent)',
    };
  }

  async function write(
    path: string,
    value: JsonValue,
    context = 'Could not change the theme',
  ): Promise<boolean> {
    const ok = value === null ? await editor.reset(path) : await editor.set(path, value);
    if (!ok) toasts.error(editor.errors[path] ?? 'write failed', context);
    return ok;
  }

  const MAX_IMPORT_BYTES = 5 * 1024 * 1024;
  let fileInput = $state<HTMLInputElement>();

  async function importTheme(file: File): Promise<void> {
    const context = 'Could not import the theme';
    if (file.size > MAX_IMPORT_BYTES) return void toasts.error('The file is larger than 5 MB', context);
    const { fromThemeFile, themeId } = await import('$lib/theme/import/terminal');
    const def = fromThemeFile(await file.text(), file.name);
    if (def instanceof Error) return void toasts.error(def.message, context);
    const id = themeId(def.name, file.name);
    const existing = settings.value()?.themes?.[id];
    if (existing) {
      const answer = await confirms.ask({
        title: `Replace "${existing.name}"?`,
        body: `It will be overwritten by "${def.name}" in config.toml.`,
        actions: [{ id: 'replace', label: 'Replace', variant: 'primary' }],
      });
      if (answer !== 'replace') return;
    }
    if (!(await write(joinPath(['themes', id]), def, context))) return;
    if (!(await write(`app.${SLOT[def.base]}`, id, context))) return;
    focused[def.base] = id;
    toasts.info(`Imported ${def.name} (used in ${def.base} mode)`);
  }

  async function remove(id: string, def: ThemeDef, list: Element | null): Promise<void> {
    const answer = await confirms.ask({
      title: `Delete "${def.name}"?`,
      body: 'It will be removed from config.toml.',
      tone: 'danger',
      actions: [{ id: 'delete', label: 'Delete', variant: 'danger' }],
    });
    if (answer !== 'delete' || !(await write(joinPath(['themes', id]), null))) return;
    for (const slot of Object.values(SLOT)) {
      if (settings.value()?.app[slot] === id) await write(`app.${slot}`, null);
    }
    focused = { dark: null, light: null };
    await tick();
    list?.querySelector<HTMLElement>('[tabindex="0"]')?.focus();
  }

  function onkey(e: KeyboardEvent, g: (typeof groups)[number], i: number): void {
    const card = g.cards[i]!;
    const last = g.cards.length - 1;
    const step: Record<string, number> = {
      ArrowRight: i + 1,
      ArrowDown: i + 1,
      ArrowLeft: i - 1,
      ArrowUp: i - 1,
      Home: 0,
      End: last,
    };
    const list = (e.currentTarget as HTMLElement).parentElement;
    if (e.key in step) {
      const next = Math.max(0, Math.min(last, step[e.key]!));
      focused[g.mode] = g.cards[next]!.id;
      (list?.children[next] as HTMLElement | undefined)?.focus();
    } else if (e.key === 'Enter' || e.key === ' ') void write(`app.${SLOT[g.mode]}`, card.id);
    else if ((e.key === 'Delete' || e.key === 'Backspace') && card.custom)
      void remove(card.id, card.def, list);
    else return;
    e.preventDefault();
  }
</script>

<div class="picker" data-testid="theme-picker">
  {#each groups as g (g.mode)}
    {@const tabId = focused[g.mode] ?? (g.missing ? g.cards[0]?.id : g.current)}
    <div class="group" data-mode={g.mode}>
      <h3 id="theme-{g.mode}-label">
        {g.mode === 'dark' ? 'Dark theme' : 'Light theme'}
        {#if settings.theme === g.mode}<Badge tone="accent">Active now</Badge>{/if}
      </h3>
      {#if g.missing}
        <p class="hint" data-testid="theme-missing">Theme "{g.current}" not found, using Bezel</p>
      {/if}
      <div class="grid" role="listbox" aria-labelledby="theme-{g.mode}-label">
        {#each g.cards as c, i (c.id)}
          {@const selected = !g.missing && c.id === g.current}
          <div
            class="card"
            class:selected
            role="option"
            aria-selected={selected}
            aria-keyshortcuts={c.custom ? 'Delete' : undefined}
            tabindex={c.id === tabId ? 0 : -1}
            data-theme-id={c.id}
            onfocus={() => (focused[g.mode] = c.id)}
            onclick={() => void write(`app.${SLOT[g.mode]}`, c.id)}
            onkeydown={(e) => onkey(e, g, i)}
          >
            <div class="top">
              <span class="name">{c.def.name}</span>
              {#if c.custom}
                <span class="tag">Custom</span>
                <IconButton
                  icon="trash-2"
                  label="Delete theme"
                  size="sm"
                  tabindex={-1}
                  onclick={(e) => {
                    e.stopPropagation();
                    void remove(c.id, c.def, e.currentTarget.closest('[role="listbox"]'));
                  }}
                />
              {/if}
              {#if selected}<span class="check"><Icon name="check" size={16} /></span>{/if}
            </div>
            <div class="strip" style:background={c.preview.well} style:color={c.preview.fg}>
              <span class="aa">Aa</span>
              <span class="dots">
                {#each c.preview.ansi as color, n (n)}<i style:background={color}></i>{/each}
              </span>
            </div>
            <div class="bar" style:background={c.preview.accent}></div>
          </div>
        {/each}
      </div>
    </div>
  {/each}
  <div class="import">
    <Button icon="folder-open" onclick={() => fileInput?.click()}>Import theme...</Button>
    <p class="help">
      VS Code (.json), iTerm2 (.itermcolors), Ghostty, Kitty (.conf), Alacritty (.toml) or base16 (.yaml).
      Tip: in VS Code, 'Developer: Generate Color Theme From Current Settings' exports the theme you use now.
    </p>
    <input
      bind:this={fileInput}
      type="file"
      hidden
      data-testid="theme-import-input"
      onchange={(e) => {
        const input = e.currentTarget;
        const file = input.files?.[0];
        input.value = ''; // picking the same file again still fires change
        if (file) void importTheme(file);
      }}
    />
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
    margin-bottom: var(--k-space-4);
  }

  h3 {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    margin: 0 0 var(--k-space-2);
    font-size: var(--k-font-size);
  }

  .hint {
    margin: 0 0 var(--k-space-2);
    color: var(--k-warn);
  }

  .import {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--k-space-2);
  }

  .help {
    margin: 0;
    color: var(--k-fg-muted);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--k-space-3);
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    padding: var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    cursor: pointer;
  }

  .card.selected {
    border-color: var(--k-accent);
    box-shadow: 0 0 0 1px var(--k-accent);
  }

  .card:focus-visible {
    outline: 2px solid var(--k-focus);
    outline-offset: 2px;
  }

  .top {
    display: flex;
    align-items: center;
    gap: var(--k-space-2);
    min-height: var(--k-control-height-sm);
  }

  .name {
    flex: 1;
    overflow: hidden;
    font-size: var(--k-font-size);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tag {
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-muted);
  }

  .check {
    display: inline-flex;
    color: var(--k-accent);
  }

  .strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 48px;
    padding: 0 var(--k-space-3);
    border-radius: var(--k-radius-sm);
  }

  .aa {
    font-size: var(--k-font-size-lg);
  }

  .dots {
    display: flex;
    gap: 4px;
  }

  .dots i {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  .bar {
    height: 3px;
    border-radius: 2px;
  }
</style>
