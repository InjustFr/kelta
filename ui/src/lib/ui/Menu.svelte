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
  }
</script>

<script lang="ts">
  import Icon from './Icon.svelte';
  import Kbd from './Kbd.svelte';

  interface Props {
    items: readonly MenuItem[];
    /** Viewport position (context menu) — the menu is clamped inside the window. */
    x: number;
    y: number;
    label?: string;
    onselect: (id: string) => void;
    onclose: () => void;
  }

  let { items, x, y, label = 'Menu', onselect, onclose }: Props = $props();

  let el = $state<HTMLDivElement>();
  let active = $state(-1);

  const enabled = $derived(items.map((it, i) => (it.disabled ? -1 : i)).filter((i) => i >= 0));

  const pos = $derived.by(() => {
    const w = el?.offsetWidth ?? 200;
    const h = el?.offsetHeight ?? items.length * 28;
    const vw = typeof window === 'undefined' ? 1e4 : window.innerWidth;
    const vh = typeof window === 'undefined' ? 1e4 : window.innerHeight;
    return { left: Math.max(4, Math.min(x, vw - w - 4)), top: Math.max(4, Math.min(y, vh - h - 4)) };
  });

  $effect(() => {
    el?.focus();
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
        // Nothing highlighted yet: Enter runs the first item (the work menu's primary action).
        choose(active >= 0 ? active : e.key === 'Enter' ? 0 : -1);
        break;
      case 'Escape':
        onclose();
        break;
      default: {
        // Letters never move: a disabled entry swallows its key.
        const i = items.findIndex((it) => it.key !== undefined && it.key === e.key);
        if (i < 0) return;
        choose(i);
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
  tabindex="-1"
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  {onkeydown}
>
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
      <span class="icon"
        >{#if item.icon}<Icon name={item.icon} size={14} />{/if}</span
      >
      <span class="label">{item.label}</span>
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
    min-width: 180px;
    max-width: 360px;
    padding: var(--k-space-2);
    border: 1px solid var(--k-border);
    border-radius: var(--k-radius);
    background: var(--k-bg-elev);
    box-shadow: var(--k-shadow);
  }

  button {
    display: flex;
    align-items: center;
    gap: var(--k-space-3);
    width: 100%;
    height: var(--k-row-height);
    padding: 0 var(--k-space-3);
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
    width: 14px;
    display: inline-flex;
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
</style>
