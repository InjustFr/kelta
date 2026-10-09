<script lang="ts">
  // Keybinding editor: effective chords per action (catalog default ⊕ `keys.bindings`), a chord
  // recorder, conflict warnings (L2's `findConflicts` for the reserved list + duplicate bindings)
  // and the compositor snippets that bind `kelta-ctl toggle`.
  import type { SettingsSectionProps } from '$app/registry';
  import { ACTIONS, RESERVED_CHORDS } from '$lib/gen/actions';
  import type { JsonValue } from '$lib/gen';
  import * as ipc from '$lib/ipc/commands';
  import { findConflicts } from '$lib/keys';
  import { toasts } from '$lib/stores';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Kbd from '$lib/ui/Kbd.svelte';
  import { currentPlatform } from '$lib/ui/format';

  import Field from '../fields/Field.svelte';
  import { useEditor } from '../lib/editor.svelte';
  import {
    chordFromEvent,
    duplicateChords,
    effectiveBindings,
    normalizeChord,
    TOGGLE_SNIPPETS,
    type BindingConflict,
  } from '../lib/chords';
  import { joinPath, isRecord } from '../lib/paths';
  import { nodeAt } from '../lib/schema';

  let props: SettingsSectionProps = $props();
  const editor = useEditor(() => ({ layer: props.layer, projectId: props.projectId, repoId: props.repoId }));
  const platform = currentPlatform();

  const overrides = $derived.by((): Record<string, string[]> => {
    const v = editor.valueOf('keys.bindings');
    return isRecord(v) ? (v as Record<string, string[]>) : {};
  });
  const prefixMap = $derived.by((): Record<string, string> => {
    const v = editor.valueOf('keys.prefix_bindings');
    return isRecord(v) ? (v as Record<string, string>) : {};
  });
  const bindings = $derived(effectiveBindings(overrides, platform));
  const access = $derived(editor.access('keys.bindings'));

  let recording = $state<string | null>(null);
  let pending = $state<{ action: string; chord: string; conflicts: BindingConflict[] } | null>(null);
  let snippet = $state<(typeof TOGGLE_SNIPPETS)[number]['id']>(platform === 'macos' ? 'macos' : 'hyprland');

  function conflictsFor(next: Record<string, string[]>): BindingConflict[] {
    const reserved = findConflicts(next, RESERVED_CHORDS);
    const dups = duplicateChords(next);
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- local scratch set, not state
    const seen = new Set<string>();
    const out: BindingConflict[] = [];
    for (const c of [...reserved, ...dups]) {
      const k = normalizeChord(c.chord);
      if (seen.has(k)) continue;
      seen.add(k);
      out.push({ chord: c.chord, actions: c.actions, reserved: c.reserved });
    }
    return out;
  }

  function startRecording(id: string): void {
    recording = id;
    pending = null;
  }

  function onkeydown(e: KeyboardEvent, action: string): void {
    if (recording !== action) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === 'Escape' && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
      recording = null;
      return;
    }
    const chord = chordFromEvent(e);
    if (!chord) return;
    recording = null;
    const next = { ...bindings, [action]: [chord] };
    const mine = conflictsFor(next).filter((c) => normalizeChord(c.chord) === normalizeChord(chord));
    pending = { action, chord, conflicts: mine };
  }

  async function apply(action: string, chords: string[]): Promise<void> {
    pending = null;
    await editor.set(joinPath(['keys', 'bindings', action]), chords as JsonValue);
  }

  async function resetBinding(action: string): Promise<void> {
    await editor.reset(joinPath(['keys', 'bindings', action]));
  }

  async function setPrefix(action: string, key: string): Promise<void> {
    const k = key.trim();
    const path = joinPath(['keys', 'prefix_bindings', action]);
    if (k === '') await editor.reset(path);
    else await editor.set(path, k);
  }

  async function copy(lines: string[]): Promise<void> {
    try {
      await ipc.clipboardWrite({ kind: 'clipboard', text: lines.join('\n') });
      toasts.info('Copied');
    } catch (err) {
      toasts.error(err, 'Copy failed');
    }
  }

  const activeSnippet = $derived(TOGGLE_SNIPPETS.find((s) => s.id === snippet) ?? TOGGLE_SNIPPETS[0]!);
  const schema = $derived(editor.schema);
</script>

<section class="keys" data-testid="keys-section">
  <p class="blurb">
    Matching uses physical keys. On Linux <code>Mod</code> is Ctrl+Shift, on macOS it is Cmd. An app prefix key
    followed by one key reaches every action.
  </p>
  {#if editor.ready && schema}
    {#each ['prefix', 'prefix_timeout_ms', 'list_keys'] as key (key)}
      {@const node = nodeAt(schema, ['keys', key])}
      {#if node}<Field path={joinPath(['keys', key])} {node} depth={1} />{/if}
    {/each}
  {/if}

  <h3>Bindings</h3>
  {#if !access.editable && access.reason}<p class="hint">{access.reason}</p>{/if}
  <table data-testid="bindings">
    <thead><tr><th>Action</th><th>Shortcut</th><th>After prefix</th><th></th></tr></thead>
    <tbody>
      {#each ACTIONS as a (a.id)}
        {@const chords = bindings[a.id] ?? []}
        {@const custom = a.id in overrides}
        <tr data-action={a.id} class:custom>
          <td>
            <div>{a.label}</div>
            <code class="id">{a.id}</code>
          </td>
          <td>
            {#if recording === a.id}
              <!-- svelte-ignore a11y_autofocus -->
              <div
                class="recorder"
                role="textbox"
                tabindex="0"
                aria-label={`Press the new shortcut for ${a.label}`}
                data-testid="recorder"
                autofocus
                onkeydown={(e) => onkeydown(e, a.id)}
                onblur={() => (recording = null)}
              >
                Press a shortcut… (Esc cancels)
              </div>
            {:else if chords.length === 0}
              <span class="muted"
                >{a.id === 'window.toggle' ? 'Bind in your compositor (below)' : 'Unbound'}</span
              >
            {:else}
              {#each chords as c (c)}<Kbd chord={c} />{/each}
            {/if}
            {#if pending && pending.action === a.id}
              <div class="pending" data-testid="pending-binding">
                <Kbd chord={pending.chord} />
                {#if pending.conflicts.length > 0}
                  <p class="warn" role="status" data-testid="conflict">
                    {#each pending.conflicts as c (c.chord)}
                      {#if c.reserved}
                        <code>{c.chord}</code> is reserved for terminal programs.
                      {:else}
                        <code>{c.chord}</code> is already used by {c.actions
                          .filter((x) => x !== a.id)
                          .join(', ')}.
                      {/if}
                    {/each}
                  </p>
                {/if}
                <Button
                  size="sm"
                  variant="primary"
                  onclick={() => pending && apply(pending.action, [pending.chord])}
                >
                  {pending.conflicts.length > 0 ? 'Bind anyway' : 'Bind'}
                </Button>
                <Button size="sm" onclick={() => (pending = null)}>Cancel</Button>
              </div>
            {/if}
          </td>
          <td>
            {#if a.prefix !== null || a.id in prefixMap}
              <input
                class="prefix"
                aria-label={`Prefix key for ${a.label}`}
                maxlength="1"
                value={prefixMap[a.id] ?? ''}
                disabled={!access.editable}
                onchange={(e) => setPrefix(a.id, e.currentTarget.value)}
              />
            {/if}
          </td>
          <td class="actions">
            {#if access.editable && a.id !== 'window.toggle'}
              <Button size="sm" onclick={() => startRecording(a.id)}>Record</Button>
              <Button size="sm" onclick={() => apply(a.id, [])} disabled={chords.length === 0}>Unbind</Button>
              {#if custom}<IconButton
                  icon="history"
                  size="sm"
                  label={`Reset ${a.label} to default`}
                  onclick={() => resetBinding(a.id)}
                />{/if}
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if editor.errors[joinPath(['keys', 'bindings'])]}<p class="error" role="alert">
      {editor.errors[joinPath(['keys', 'bindings'])]}
    </p>{/if}

  <h3>Global toggle</h3>
  <p class="blurb">
    Kelta registers no global shortcut. Bind <code>kelta-ctl toggle</code> in your compositor or desktop:
  </p>
  <div class="tabs" role="tablist">
    {#each TOGGLE_SNIPPETS as s (s.id)}
      <button type="button" role="tab" aria-selected={snippet === s.id} onclick={() => (snippet = s.id)}
        >{s.label}</button
      >
    {/each}
  </div>
  <pre class="snippet" data-testid="snippet">{activeSnippet.lines.join('\n')}</pre>
  <Button size="sm" icon="copy" onclick={() => copy([...activeSnippet.lines])}>Copy</Button>
</section>

<style>
  .keys {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-3);
  }

  .blurb,
  .hint {
    margin: 0;
    color: var(--k-fg-muted);
  }

  h3 {
    margin: var(--k-space-4) 0 0;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    text-align: left;
    vertical-align: top;
    padding: var(--k-space-2) var(--k-space-3);
    border-bottom: 1px solid var(--k-border);
  }

  tr.custom td:first-child {
    border-left: 2px solid var(--k-accent);
  }

  .id {
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .muted {
    color: var(--k-fg-subtle);
  }

  .actions {
    white-space: nowrap;
    text-align: right;
  }

  .prefix {
    width: 32px;
    height: var(--k-control-height);
    text-align: center;
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
    font-family: var(--k-font-mono);
  }

  .recorder {
    display: inline-block;
    padding: var(--k-space-2) var(--k-space-4);
    border: 2px solid var(--k-accent);
    border-radius: var(--k-radius);
    outline: none;
  }

  .pending {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--k-space-3);
    margin-top: var(--k-space-2);
  }

  .warn {
    margin: 0;
    color: var(--k-warn);
  }

  .tabs {
    display: flex;
    gap: var(--k-space-2);
  }

  .tabs button {
    padding: var(--k-space-2) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: transparent;
    color: var(--k-fg-muted);
    cursor: pointer;
  }

  .tabs button[aria-selected='true'] {
    background: var(--k-bg-selected);
    color: var(--k-fg);
  }

  .snippet {
    margin: 0;
    padding: var(--k-space-3) var(--k-space-4);
    background: var(--k-bg-sunken);
    border-radius: var(--k-radius);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    overflow-x: auto;
  }

  .error {
    color: var(--k-danger);
  }
</style>
