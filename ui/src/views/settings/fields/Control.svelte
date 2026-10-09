<script lang="ts">
  // Value-based editor for one schema node. `onchange` fires when the user commits an edit
  // (change/blur/Enter, toggle, select); a `null` result means "unset".
  import type { JsonValue } from '$lib/gen';
  import Button from '$lib/ui/Button.svelte';
  import IconButton from '$lib/ui/IconButton.svelte';
  import Select from '$lib/ui/Select.svelte';
  import TextInput from '$lib/ui/TextInput.svelte';
  import Toggle from '$lib/ui/Toggle.svelte';

  import { clone, equal, isRecord } from '../lib/paths';
  import {
    childEntries,
    defaultOf,
    descriptionOf,
    entrySchema,
    enumOptions,
    fieldKind,
    resolve,
    titleOf,
    variantOptions,
    type SchemaNode,
  } from '../lib/schema';
  import Control from './Control.svelte';
  import SecretControl from './SecretControl.svelte';

  interface Props {
    node: SchemaNode;
    value: JsonValue | undefined;
    onchange: (value: JsonValue | null) => void;
    label: string;
    readonly?: boolean;
    /** Show "(unset)" for optional fields. */
    template?: boolean;
    id?: string;
  }

  let { node, value, onchange, label, readonly = false, template = false, id }: Props = $props();

  const resolved = $derived(resolve(node));
  const kind = $derived(fieldKind(node));
  const n = $derived(resolved.node);

  // ---- text / number drafts ----------------------------------------------------------------
  let text = $state('');
  let localError = $state<string | null>(null);

  $effect(() => {
    if (kind === 'string' || kind === 'secret') text = typeof value === 'string' ? value : '';
    else if (kind === 'int' || kind === 'float')
      text = value === undefined || value === null ? '' : String(value);
    else if (kind === 'json') text = value === undefined ? '' : JSON.stringify(value, null, 2);
    localError = null;
  });

  function commitText(): void {
    if (text === (typeof value === 'string' ? value : '')) return;
    if (text === '' && resolved.nullable) onchange(null);
    else onchange(text);
  }

  function commitNumber(): void {
    const raw = text.trim();
    if (raw === '') {
      localError = null;
      if (resolved.nullable) onchange(null);
      return;
    }
    const num = Number(raw);
    if (!Number.isFinite(num)) {
      localError = 'Enter a number';
      return;
    }
    if (kind === 'int' && !Number.isInteger(num)) {
      localError = 'Enter a whole number';
      return;
    }
    if (n.minimum !== undefined && num < n.minimum) {
      localError = `Must be at least ${n.minimum}`;
      return;
    }
    if (n.maximum !== undefined && num > n.maximum) {
      localError = `Must be at most ${n.maximum}`;
      return;
    }
    localError = null;
    if (num !== value) onchange(num);
  }

  function commitJson(): void {
    const raw = text.trim();
    if (raw === '') {
      onchange(null);
      return;
    }
    try {
      const parsed = JSON.parse(raw) as JsonValue;
      localError = null;
      if (!equal(parsed, value)) onchange(parsed);
    } catch (err) {
      localError = `Invalid JSON: ${(err as Error).message}`;
    }
  }

  // ---- lists / maps --------------------------------------------------------------------------
  let chip = $state('');
  const list = $derived(Array.isArray(value) ? (value as JsonValue[]).map(String) : []);

  function addChip(): void {
    const v = chip.trim();
    if (!v) return;
    chip = '';
    if (!list.includes(v)) onchange([...list, v]);
  }

  function removeChip(i: number): void {
    onchange(list.filter((_, j) => j !== i));
  }

  const mapEntries = $derived(isRecord(value) ? Object.entries(value) : []);
  let newKey = $state('');

  function setMapEntry(key: string, v: JsonValue): void {
    const next = { ...(isRecord(value) ? value : {}) };
    next[key] = v;
    onchange(next);
  }

  function renameMapEntry(oldKey: string, newName: string): void {
    const name = newName.trim();
    if (!name || name === oldKey || !isRecord(value)) return;
    const next: { [k: string]: JsonValue } = {};
    for (const [k, v] of Object.entries(value)) next[k === oldKey ? name : k] = v;
    onchange(next);
  }

  function removeMapEntry(key: string): void {
    if (!isRecord(value)) return;
    const next = { ...value };
    delete next[key];
    onchange(next);
  }

  function addMapEntry(): void {
    const k = newKey.trim();
    if (!k || (isRecord(value) && k in value)) return;
    newKey = '';
    const vs = entrySchema(node);
    const init: JsonValue =
      kind === 'list-map' ? [] : kind === 'enum-map' ? (enumOptions(vs ?? {})[0]?.value ?? '') : '';
    setMapEntry(k, init);
  }

  // ---- group -----------------------------------------------------------------------------------
  function setChild(key: string, v: JsonValue | null): void {
    const next = clone(isRecord(value) ? value : {});
    if (v === null) delete next[key];
    else next[key] = v;
    onchange(next);
  }

  // ---- variant (`{ category = … }` | `{ name = … }`) -------------------------------------------
  const variants = $derived(kind === 'variant' ? variantOptions(node) : []);
  const activeVariant = $derived(isRecord(value) ? (variants.find(([k]) => k in value)?.[0] ?? null) : null);

  function pickVariant(key: string): void {
    if (key === '') {
      onchange(null);
      return;
    }
    const schema = variants.find(([k]) => k === key)?.[1];
    onchange({ [key]: schema ? defaultOf(schema) : '' });
  }

  const enumOpts = $derived(kind === 'enum' ? enumOptions(node) : []);
  const currentEnum = $derived(enumOpts.find((o) => o.value === value));
  const idAttr = $derived(id);
</script>

<div class="control" data-kind={kind}>
  {#if kind === 'bool'}
    <Toggle checked={value === true} {label} disabled={readonly} onchange={(c) => onchange(c)} />
  {:else if kind === 'enum'}
    <Select
      id={idAttr}
      value={typeof value === 'string' ? value : undefined}
      options={[
        ...(resolved.nullable || value === undefined || value === null
          ? [{ value: '', label: '(unset)' }]
          : []),
        ...enumOpts,
      ]}
      disabled={readonly}
      onchange={(v) => onchange(v === '' ? null : v)}
    />
    {#if currentEnum?.description}<p class="hint">{currentEnum.description}</p>{/if}
  {:else if kind === 'int' || kind === 'float'}
    <input
      id={idAttr}
      class="num"
      type="number"
      aria-label={label}
      value={text}
      oninput={(e) => (text = e.currentTarget.value)}
      min={n.minimum}
      max={n.maximum}
      step={kind === 'int' ? 1 : 'any'}
      disabled={readonly}
      onchange={commitNumber}
    />
  {:else if kind === 'secret'}
    <SecretControl
      value={typeof value === 'string' ? value : ''}
      {readonly}
      onchange={(ref) => onchange(ref)}
    />
  {:else if kind === 'string'}
    <TextInput
      id={idAttr}
      bind:value={text}
      aria-label={label}
      disabled={readonly}
      spellcheck={false}
      class={template ? 'template' : ''}
      onchange={commitText}
    />
  {:else if kind === 'string-list'}
    <div class="chips" data-testid="chips">
      {#each list as item, i (item + i)}
        <span class="chip">
          <span class="chip-text">{item}</span>
          {#if !readonly}<IconButton
              icon="x"
              label={`Remove ${item}`}
              size="sm"
              onclick={() => removeChip(i)}
            />{/if}
        </span>
      {/each}
      {#if !readonly}
        <input
          class="chip-input"
          aria-label={`Add to ${label}`}
          placeholder="Add…"
          bind:value={chip}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ',') {
              e.preventDefault();
              addChip();
            }
          }}
          onblur={addChip}
        />
      {/if}
    </div>
  {:else if kind === 'string-map' || kind === 'enum-map' || kind === 'list-map'}
    <div class="rows" data-testid="map-rows">
      {#each mapEntries as [k, v] (k)}
        <div class="row">
          <input
            class="key"
            aria-label="Key"
            value={k}
            disabled={readonly}
            onchange={(e) => renameMapEntry(k, e.currentTarget.value)}
          />
          {#if kind === 'string-map'}
            <input
              class="val"
              aria-label={`Value of ${k}`}
              value={String(v)}
              disabled={readonly}
              onchange={(e) => setMapEntry(k, e.currentTarget.value)}
            />
          {:else if kind === 'enum-map'}
            <Select
              value={String(v)}
              options={enumOptions(entrySchema(node) ?? {})}
              disabled={readonly}
              onchange={(x) => setMapEntry(k, x)}
            />
          {:else}
            <input
              class="val"
              aria-label={`Values of ${k}`}
              value={Array.isArray(v) ? v.join(', ') : ''}
              disabled={readonly}
              placeholder="comma separated"
              onchange={(e) =>
                setMapEntry(
                  k,
                  e.currentTarget.value
                    .split(',')
                    .map((s) => s.trim())
                    .filter(Boolean),
                )}
            />
          {/if}
          {#if !readonly}<IconButton
              icon="trash-2"
              label={`Remove ${k}`}
              size="sm"
              onclick={() => removeMapEntry(k)}
            />{/if}
        </div>
      {/each}
      {#if !readonly}
        <div class="row">
          <input
            class="key"
            aria-label="New key"
            placeholder="New key"
            bind:value={newKey}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addMapEntry();
              }
            }}
          />
          <Button size="sm" icon="plus" onclick={addMapEntry} disabled={!newKey.trim()}>Add</Button>
        </div>
      {/if}
    </div>
  {:else if kind === 'group'}
    <div class="group">
      {#each childEntries(node) as [key, child] (key)}
        <div class="subfield">
          {#if fieldKind(child) !== 'bool'}<span class="sublabel">{titleOf(key)}</span>{/if}
          <Control
            node={child}
            value={isRecord(value) ? value[key] : undefined}
            label={titleOf(key)}
            {readonly}
            onchange={(v) => setChild(key, v)}
          />
          {#if descriptionOf(child)}<p class="hint">{descriptionOf(child)}</p>{/if}
        </div>
      {/each}
    </div>
  {:else if kind === 'variant'}
    <div class="variant">
      <Select
        value={activeVariant ?? ''}
        options={[
          { value: '', label: '(unset)' },
          ...variants.map(([k]) => ({ value: k, label: titleOf(k) })),
        ]}
        disabled={readonly}
        onchange={pickVariant}
      />
      {#if activeVariant}
        {@const vs = variants.find(([k]) => k === activeVariant)?.[1]}
        {#if vs && isRecord(value)}
          <Control
            node={vs}
            value={value[activeVariant]}
            label={titleOf(activeVariant)}
            {readonly}
            onchange={(v) => onchange(v === null ? null : { [activeVariant]: v })}
          />
        {/if}
      {/if}
    </div>
  {:else}
    <textarea
      id={idAttr}
      class="json"
      rows="4"
      spellcheck="false"
      aria-label={label}
      bind:value={text}
      disabled={readonly}
      onchange={commitJson}></textarea>
  {/if}
  {#if localError}<p class="error" role="alert">{localError}</p>{/if}
</div>

<style>
  .control {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    min-width: 0;
  }

  .num {
    width: 140px;
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
  }

  .num:focus,
  .json:focus,
  .rows input:focus,
  .chip-input:focus {
    border-color: var(--k-focus);
  }

  :global(.template input) {
    font-family: var(--k-font-mono);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--k-space-2);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-1);
    padding: 0 var(--k-space-1) 0 var(--k-space-3);
    height: 22px;
    border-radius: 11px;
    background: var(--k-bg-sunken);
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
  }

  .chip-input {
    height: 22px;
    min-width: 90px;
    padding: 0 var(--k-space-3);
    border: 1px dashed var(--k-border-strong);
    border-radius: 11px;
    background: transparent;
    color: var(--k-fg);
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
  }

  .rows input {
    height: var(--k-control-height);
    padding: 0 var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
    min-width: 0;
  }

  .key {
    width: 180px;
    font-family: var(--k-font-mono);
  }

  .val {
    flex: 1;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-4);
    padding: var(--k-space-3) var(--k-space-4);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
  }

  .subfield {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
  }

  .sublabel {
    font-size: var(--k-font-size-sm);
    color: var(--k-fg-muted);
  }

  .variant {
    display: flex;
    gap: var(--k-space-3);
    align-items: flex-start;
    flex-wrap: wrap;
  }

  .json {
    font-family: var(--k-font-mono);
    font-size: var(--k-font-size-sm);
    padding: var(--k-space-3);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg);
    color: var(--k-fg);
    resize: vertical;
  }

  .hint {
    margin: 0;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .error {
    margin: 0;
    font-size: var(--k-font-size-xs);
    color: var(--k-danger);
  }
</style>
