<script lang="ts">
  // Settings > Tools quick form: appends a plain `[[tools]]` entry (Terminal = pty, App = external
  // `open -a`) to the global layer (tools are personal). Everything else stays in the full editor below.
  import type { JsonValue, ToolInfo } from '$lib/gen';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Kbd from '$lib/ui/Kbd.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';
  import { currentPlatform } from '$lib/ui/format';
  import { ICON_NAMES } from '$lib/ui/icons';

  import type { SettingsEditor } from '../../settings/lib/editor.svelte';
  import { chordFromEvent } from '../../settings/lib/chords';
  import { splitArgv, toolKeyConflict, uniqueToolId } from '../../settings/lib/tools';

  interface Props {
    editor: SettingsEditor;
    /** Merged tool list: global + plugin tools (ids are deduped against it and the global layer). */
    merged: readonly ToolInfo[] | undefined;
    bindings: Record<string, string[]>;
    open: boolean;
  }

  let { editor, merged, bindings, open = $bindable() }: Props = $props();
  const platform = currentPlatform();

  let kind = $state<'terminal' | 'app'>('terminal');
  let name = $state('');
  let commandLine = $state('');
  let app = $state('');
  let openFolder = $state(true);
  let key = $state('');
  let icon = $state('');

  const argv = $derived(splitArgv(commandLine));
  // tool_list drops disabled stubs, so the edited layer's own ids count too (kelta-config rejects duplicates).
  const own = $derived.by(() => {
    const v = editor.layerValue('tools');
    return Array.isArray(v) ? v : [];
  });
  const id = $derived(
    uniqueToolId(name, [
      ...(merged ?? []).map((t) => t.id),
      ...own.map((t) => (t as { id?: string } | null)?.id ?? ''),
    ]),
  );
  const conflict = $derived(key ? toolKeyConflict(key, id, bindings, merged ?? [], platform) : null);
  // The layer document must be loaded: the write replaces the layer's whole `tools` array.
  // The merged list must be loaded too, or an inherited id would not be deduped.
  const ready = $derived(
    editor.doc !== null &&
      merged !== undefined &&
      name.trim() !== '' &&
      (kind === 'app' ? app.trim() !== '' : argv !== null && !!argv[0]),
  );
  const kinds = [
    { id: 'terminal' as const, label: 'Terminal' },
    ...(platform === 'macos' ? [{ id: 'app' as const, label: 'App' }] : []),
  ];
  const icons = [{ value: '', label: 'None' }, ...ICON_NAMES.map((n) => ({ value: n, label: n }))];

  function record(e: KeyboardEvent): void {
    if (e.key === 'Tab' || e.key === 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    const chord = chordFromEvent(e);
    if (chord) key = chord;
  }

  async function add(): Promise<void> {
    if (!ready) return;
    const entry: Record<string, JsonValue> = { id, label: name.trim() };
    if (kind === 'app') {
      Object.assign(entry, {
        kind: 'external',
        command: 'open',
        args: ['-a', app.trim(), ...(openFolder ? ['.'] : [])],
      });
    } else {
      entry.command = argv![0]!;
      if (argv!.length > 1) entry.args = argv!.slice(1);
    }
    if (key) entry.keybinding = key;
    if (icon) entry.icon = icon;
    if (await editor.set('tools', [...own, entry])) {
      name = commandLine = app = key = icon = '';
      open = false;
    }
  }
</script>

{#if open}
  <form
    class="add"
    data-testid="add-tool"
    onsubmit={(e) => {
      e.preventDefault();
      void add();
    }}
  >
    <Tabs items={kinds} bind:value={kind} label="Tool kind" />
    <TextInput
      label="Name"
      bind:value={name}
      placeholder="lazygit"
      hint={name.trim() ? `id: ${id}` : undefined}
    />
    {#if kind === 'terminal'}
      <TextInput
        label="Command line"
        bind:value={commandLine}
        placeholder="lazygit"
        error={argv === null ? 'A quote is not closed' : null}
      />
    {:else}
      <TextInput label="Application" bind:value={app} placeholder="Fork" />
      <Toggle label="Open the current folder in it" bind:checked={openFolder} />
    {/if}
    <div class="key">
      <span class="label" id="add-tool-key">Key (optional)</span>
      <div
        class="recorder"
        role="textbox"
        tabindex="0"
        aria-labelledby="add-tool-key"
        data-testid="tool-key"
        onkeydown={record}
      >
        {#if key}<Kbd chord={key} />{:else}<span class="muted">Focus and press a shortcut</span>{/if}
      </div>
      {#if key}<IconButton icon="x" size="sm" label="Clear key" onclick={() => (key = '')} />{/if}
    </div>
    {#if conflict}<p class="warn" role="status" data-testid="tool-key-conflict">{conflict}</p>{/if}
    <Select label="Icon (optional)" options={icons} bind:value={icon} />
    {#if editor.errors['tools']}<p class="error" role="alert">{editor.errors['tools']}</p>{/if}
    <div class="buttons">
      <Button type="submit" size="sm" variant="primary" disabled={!ready}>Add</Button>
      <Button size="sm" variant="ghost" onclick={() => (open = false)}>Cancel</Button>
    </div>
  </form>
{:else}
  <div><Button size="sm" icon="plus" onclick={() => (open = true)}>Add tool</Button></div>
{/if}

<style>
  .add {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
    max-width: 480px;
    padding: var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .key {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .label {
    font-size: var(--k-font-size-sm);
  }

  .recorder {
    min-width: 160px;
    padding: var(--k-space-2) var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    outline: none;
  }

  .recorder:focus {
    border-color: var(--k-accent);
  }

  .muted {
    color: var(--k-fg-subtle);
  }

  .warn {
    margin: 0;
    color: var(--k-warn);
  }

  .error {
    margin: 0;
    color: var(--k-danger);
  }

  .buttons {
    display: flex;
    gap: var(--k-space-2);
  }
</style>
