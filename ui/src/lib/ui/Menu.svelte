<script lang="ts" module>
  export interface MenuItem {
    id: string;
    label: string;
    icon?: string;
    /** Chord shown on the right. */
    kbd?: string;
    disabled?: boolean;
    danger?: boolean;
    /** Render a separator before this item. */
    separator?: boolean;
    /** Single key that runs the item while the menu is open (`d`, `F` = Shift+F). */
    key?: string;
    /** Tooltip (a disabled item's reason). */
    title?: string;
    /** Secondary text after the label, in the subtle colour. */
    detail?: string;
    /** Colour of a 2px category bar shown in place of the icon. */
    bar?: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';

  import Icon from './Icon.svelte';
  import Kbd from './Kbd.svelte';

  interface Props {
    items: readonly MenuItem[];
    /** Viewport position (context menu) — the menu is clamped inside the window. */
    x: number;
    y: number;
    label?: string;
    /** Content above the items; it also describes the menu (`aria-describedby`). */
    header?: Snippet;
    /** A filter in the header narrows the items: the first enabled one stays lit as Enter's target. */
    filtered?: boolean;
    onselect: (id: string) => void;
    onclose: () => void;
  }

  let { items, x, y, label = 'Menu', header, filtered = false, onselect, onclose }: Props = $props();
  const headerId = $props.id();

  let el = $state<HTMLDivElement>();
  let active = $state(-1);

  const enabled = $derived(items.map((it, i) => (it.disabled ? -1 : i)).filter((i) => i >= 0));

  const pos = $derived.by(() => {
    const w = el?.offsetWidth ?? 200;
    const h = el?.offsetHeight ?? items.length * 32;
    const vw = typeof window === 'undefined' ? 1e4 : window.innerWidth;
    const vh = typeof window === 'undefined' ? 1e4 : window.innerHeight;
    return { left: Math.max(4, Math.min(x, vw - w - 4)), top: Math.max(4, Math.min(y, vh - h - 4)) };
  });

  $effect(() => {
    el?.focus();
  });

  $effect(() => {
    if (filtered) active = enabled[0] ?? -1;
  });

  function move(delta: number): void {
    if (enabled.length === 0) return;
    const at = enabled.indexOf(active);
    const next =
      at < 0 ? (delta > 0 ? 0 : enabled.length - 1) : (at + delta + enabled.length) % enabled.length;
    active = enabled[next] ?? -1;
  }

  function choose(i: number): void {
    const item = items[i];
    if (!item || item.disabled) return;
    onselect(item.id);
    onclose();
  }

  function onkeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case 'ArrowDown':
        move(1);
        break;
      case 'ArrowUp':
        move(-1);
        break;
      case 'Enter':
      case ' ':
        // Nothing highlighted (or a filter shrank the list under it): Enter runs the first item.
        choose(items[active] ? active : e.key === 'Enter' ? 0 : -1);
        break;
      case 'Escape':
        onclose();
        break;
      default: {
        // Letters never move: a disabled entry swallows its key.
        const i = items.findIndex((it) => it.key !== undefined && it.key === e.key);
        if (i >= 0) choose(i);
        else if (e.key === 'j' || e.key === 'k')
          move(e.key === 'j' ? 1 : -1); // unless an item claims them
        else return;
      }
    }
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="k-menu-backdrop"
  onpointerdown={onclose}
  oncontextmenu={(e) => (e.preventDefault(), onclose())}
></div>
<div
  bind:this={el}
  class="k-menu"
  role="menu"
  aria-label={label}
  aria-describedby={header ? headerId : undefined}
  tabindex="-1"
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  {onkeydown}
>
  {#if header}<div id={headerId}>{@render header()}</div>{/if}
  {#each items as item, i (item.id)}
    {#if item.separator}<div class="sep" role="separator"></div>{/if}
    <button
      type="button"
      role="menuitem"
      class:active={i === active}
      class:danger={item.danger}
      class:off={item.disabled}
      aria-disabled={item.disabled ? 'true' : undefined}
      title={item.title}
      onpointerenter={() => (active = item.disabled ? -1 : i)}
      onclick={() => choose(i)}
    >
      {#if item.bar}<span class="bar" style:--bar={item.bar}></span>{:else}<span class="icon"
          >{#if item.icon}<Icon name={item.icon} size={16} />{/if}</span
        >{/if}
      <span class="label">{item.label}</span>
      {#if item.detail}<span class="detail">{item.detail}</span>{/if}
      {#if item.kbd}<Kbd chord={item.kbd} />{/if}
    </button>
  {/each}
</div>

<style>
  .k-menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--k-z-menu);
  }

  .k-menu {
    position: fixed;
    z-index: var(--k-z-menu);
    min-width: 220px;
    max-width: 400px;
    padding: var(--k-space-2);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-float-in var(--k-duration) ease-out;
  }

  button {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 100%;
    height: var(--k-row-height);
    padding: 0 var(--k-space-4);
    border: none;
    border-radius: var(--k-radius-sm);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  button.active {
    background: var(--k-bg-selected);
  }

  button.danger {
    color: var(--k-danger);
  }

  button.off {
    opacity: 0.5;
    cursor: default;
  }

  .icon {
    width: 16px;
    display: inline-flex;
  }

  .bar {
    flex: none;
    width: 2px;
    height: 14px;
    background: var(--bar);
  }

  .detail {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sep {
    height: 1px;
    margin: var(--k-space-2) 0;
    background: var(--k-border);
  }

  button :global(.k-kbd) {
    margin-left: auto;
    justify-content: flex-end;
    color: var(--k-fg-muted);
  }
</style>
