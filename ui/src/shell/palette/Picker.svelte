<script lang="ts" module>
  import type { LampLevel } from '$lib/stores';

  export interface PickerItem {
    id: string;
    label: string;
    detail?: string;
    icon?: string;
    kbd?: string;
    group?: string;
    lamp?: LampLevel;
    value?: string;
    /** An on/off setting: a switch, ↵ flips it, ⌘↵ opens it in Settings. */
    checked?: boolean;
  }
</script>

<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { Icon, Kbd, Lamp, Sheet, Spinner } from '$lib/ui';

  interface Props {
    title: string;
    placeholder: string;
    items: readonly PickerItem[];
    query: string;
    onquery: (query: string) => void;
    onpick: (id: string, alt?: boolean) => void;
    onclose: () => void;
    empty?: string;
    busy?: boolean;
    testid?: string;
  }

  let {
    title,
    placeholder,
    items,
    query,
    onquery,
    onpick,
    onclose,
    empty,
    busy = false,
    testid = 'picker',
  }: Props = $props();

  // One header per group: groups keep the order of their best hit, items keep score order inside.
  const shown = $derived.by(() => {
    const order = [...new Set(items.map((i) => i.group))];
    return [...items].sort((a, b) => order.indexOf(a.group) - order.indexOf(b.group));
  });

  let selected = $state(0);
  let list = $state<HTMLElement>();
  const current = $derived(shown[selected]);

  $effect(() => {
    // New results: select the first one.
    void shown;
    selected = 0;
  });

  $effect(() => {
    const el = list?.querySelector<HTMLElement>(`[data-index="${selected}"]`);
    el?.scrollIntoView({ block: 'nearest' });
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
      e.preventDefault();
      selected = shown.length === 0 ? 0 : (selected + 1) % shown.length;
    } else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
      e.preventDefault();
      selected = shown.length === 0 ? 0 : (selected - 1 + shown.length) % shown.length;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const item = shown[selected];
      if (item) onpick(item.id, e.metaKey || e.ctrlKey);
    }
  }
</script>

<Sheet {title} side="top" width={640} {onclose}>
  <div class="picker" data-testid={testid}>
    <div class="input">
      <Icon name="search" size={16} />
      <input
        type="text"
        {placeholder}
        value={query}
        oninput={(e) => onquery(e.currentTarget.value)}
        {onkeydown}
        spellcheck="false"
        autocomplete="off"
        aria-label={title}
        data-testid="{testid}-input"
      />
      {#if busy}<Spinner size={16} />{/if}
    </div>
    <div class="list" bind:this={list} role="listbox" aria-label={t('Results')}>
      {#each shown as item, i (item.id)}
        {#if item.group && item.group !== shown[i - 1]?.group}
          <div class="group" role="presentation">{t(item.group)}</div>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="row"
          class:selected={i === selected}
          role="option"
          tabindex="-1"
          aria-selected={i === selected}
          aria-checked={item.checked}
          data-index={i}
          data-testid="{testid}-item"
          onpointermove={() => (selected = i)}
          onclick={(e) => onpick(item.id, e.metaKey || e.ctrlKey)}
        >
          <span class="lamp-slot"
            >{#if item.lamp}<Lamp level={item.lamp} />{/if}</span
          >
          <Icon name={item.icon ?? 'command'} size={16} />
          <span class="label">{item.label}</span>
          {#if item.detail}<span class="detail">{item.detail}</span>{/if}
          {#if item.value}<span class="value">{item.value}</span>{/if}
          {#if item.checked !== undefined}<span class="switch" class:on={item.checked} aria-hidden="true"></span>{/if}
          {#if item.kbd}<Kbd chord={item.kbd} />{/if}
        </div>
      {:else}
        <p class="empty">{empty ?? t('No results')}</p>
      {/each}
    </div>
    <footer class="hints">
      <span><Kbd chord="up" /><Kbd chord="down" /> {t('Move')}</span>
      {#if current?.checked !== undefined}
        <span><Kbd chord="enter" /> {t('Toggle')}</span>
        <span><Kbd chord="mod+enter" /> {t('Open in Settings')}</span>
      {:else}
        <span><Kbd chord="enter" /> {t('Run')}</span>
      {/if}
      <span><Kbd chord="escape" /> {t('Close')}</span>
    </footer>
  </div>
</Sheet>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--k-space-2);
    margin: calc(var(--k-space-4) * -1) calc(var(--k-space-5) * -1);
  }

  .input {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: 48px;
    padding: 0 var(--k-space-5);
    border-bottom: 1px solid var(--k-border);
    color: var(--k-fg-subtle);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: none;
    background: transparent;
    color: var(--k-fg);
    font-family: var(--k-font-ui);
    font-size: var(--k-font-size-lg);
  }

  /* The ring moves to the input row: an accent underline while the field has focus. */
  input:focus-visible {
    outline: none;
  }

  .input:focus-within {
    box-shadow: inset 0 -2px 0 var(--k-focus);
  }

  .list {
    max-height: 52vh;
    overflow-y: auto;
    padding: 0 var(--k-space-2) var(--k-space-2);
  }

  .group {
    padding: var(--k-space-3) var(--k-space-3) var(--k-space-1);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    font-weight: var(--k-weight-medium);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    height: var(--k-row-height);
    padding: 0 var(--k-space-3);
    border-radius: var(--k-radius-sm);
    color: var(--k-fg);
    cursor: pointer;
  }

  .lamp-slot {
    display: inline-flex;
    flex: none;
    width: 12px;
  }

  .row > :global(.k-icon) {
    color: var(--k-fg-subtle);
  }

  .row.selected {
    background: var(--k-bg-selected);
    box-shadow: inset 2px 0 0 var(--k-accent);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--k-fg-subtle);
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-sm);
  }

  .value {
    flex: none;
    max-width: 30%;
    overflow: hidden;
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Toggle's track, drawn only: ↵ flips it. */
  .switch {
    position: relative;
    flex: none;
    width: 28px;
    height: 16px;
    border-radius: 8px;
    background: var(--k-bezel);
    box-shadow: inset 0 0 0 1px var(--k-border-strong);
  }

  .switch::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--k-fg-subtle);
    transition: translate var(--k-duration-fast) var(--k-ease);
  }

  .switch.on {
    background: var(--k-accent);
    box-shadow: none;
  }

  .switch.on::after {
    translate: 12px 0;
    background: var(--k-well);
  }

  .row :global(.k-kbd) {
    margin-left: auto;
  }

  .hints {
    display: flex;
    gap: var(--k-space-5);
    padding: var(--k-space-3) var(--k-space-5);
    border-top: 1px solid var(--k-border);
    color: var(--k-fg-muted);
    font-size: var(--k-font-size-xs);
  }

  .hints span {
    display: inline-flex;
    align-items: center;
    gap: var(--k-space-2);
  }

  .empty {
    margin: 0;
    padding: var(--k-space-4) var(--k-space-3);
    color: var(--k-fg-muted);
  }
</style>
