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
    /** Header of the group this item starts (#439), shown before it with a separator. */
    group?: string;
    /** Single key that runs the item while the menu is open (`d`, `F` = Shift+F). */
    key?: string;
    /** Tooltip (a disabled item's reason). */
    title?: string;
    /** Secondary text after the label, in the subtle colour. */
    detail?: string;
    /** Colour of a 2px category bar shown in place of the icon. */
    bar?: string;
    /** Why it cannot run now (#405): listed under "Not available now", the reason on a second line. */
    reason?: string | null;
    /** With a reason: not listed at all (it never applies here); its key still flashes the reason. */
    hidden?: boolean;
  }

  /** At most this many entries under "Not available now", the first ones given (#405). */
  export const MAX_OFF = 5;

  /** What a menu lists: the entries that can run, then those that cannot (the "Not available now" group). */
  export function menuView(items: readonly MenuItem[]): { now: MenuItem[]; off: MenuItem[] } {
    return {
      now: items.filter((it) => !it.reason && !it.hidden),
      off: items.filter((it) => it.reason && !it.hidden).slice(0, MAX_OFF),
    };
  }
</script>

<script lang="ts">
  import { t } from '$lib/i18n/index.svelte';
  import { toasts } from '$lib/stores';
  import type { Snippet } from 'svelte';

  import Icon from './Icon.svelte';
  import Kbd from './Kbd.svelte';
  import { fadeMove } from './motion';

  interface Props {
    items: readonly MenuItem[];
    /** Viewport position (context menu) — the menu is clamped inside the window. */
    x: number;
    y: number;
    label?: string;
    /** Content above the items; it also describes the menu (`aria-describedby`). */
    header?: Snippet;
    /** Content under the items (a hint, #479). */
    footer?: Snippet;
    /** A filter in the header narrows the items: the first enabled one stays lit as Enter's target. */
    filtered?: boolean;
    /** Opened from the keyboard (`⌘.`, #404): the first entry, the next step, starts lit. */
    lit?: boolean;
    onselect: (id: string) => void;
    onclose: () => void;
  }

  let {
    items,
    x,
    y,
    label,
    header,
    footer,
    filtered = false,
    lit = false,
    onselect,
    onclose,
  }: Props = $props();
  const headerId = $props.id();

  let el = $state<HTMLDivElement>();
  // svelte-ignore state_referenced_locally
  let active = $state(lit ? 0 : -1);

  const view = $derived(menuView(items));
  /** The listed entries, in order; `active` indexes this. */
  const list = $derived([...view.now, ...view.off]);
  const enabled = $derived(list.map((it, i) => (it.disabled || it.reason ? -1 : i)).filter((i) => i >= 0));

  const pos = $derived.by(() => {
    const w = el?.offsetWidth ?? 200;
    const h = el?.offsetHeight ?? list.length * 32;
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

  // A long menu scrolls: keep the lit entry in view while the arrows move it.
  $effect(() => {
    if (active >= 0)
      el?.querySelectorAll<HTMLElement>('[role="menuitem"]')[active]?.scrollIntoView?.({ block: 'nearest' });
  });

  function move(delta: number): void {
    if (enabled.length === 0) return;
    const at = enabled.indexOf(active);
    const next =
      at < 0 ? (delta > 0 ? 0 : enabled.length - 1) : (at + delta + enabled.length) % enabled.length;
    active = enabled[next] ?? -1;
  }

  function choose(i: number): void {
    const item = list[i];
    if (!item || item.disabled || item.reason) return;
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
        choose(list[active] ? active : e.key === 'Enter' ? 0 : -1);
        break;
      case 'Escape':
        onclose();
        break;
      default: {
        // Letters never move: a disabled entry swallows its key, one that cannot run now flashes why.
        const it = items.find((x) => x.key !== undefined && x.key === e.key);
        if (it?.reason) toasts.info(`${it.label}: ${it.reason}`);
        else if (it) choose(list.indexOf(it));
        else if (e.key === 'j' || e.key === 'k')
          move(e.key === 'j' ? 1 : -1); // unless an item claims them
        else {
          // Not ours, but not the pane's either: a letter typed in an open menu must not reach its shortcuts.
          e.stopPropagation();
          return;
        }
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
  aria-label={label ?? t('Menu')}
  aria-describedby={header ? headerId : undefined}
  tabindex="-1"
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  style:transform-origin="{x - pos.left}px {y - pos.top}px"
  {onkeydown}
  out:fadeMove
>
  {#if header}<div id={headerId}>{@render header()}</div>{/if}
  {#each list as item, i (item.id)}
    {#if item.reason}
      {#if item === view.off[0]}
        {#if i > 0}<div class="sep" role="separator"></div>{/if}
        <div class="group" role="presentation">{t('Not available now ({n})', { n: view.off.length })}</div>
      {/if}
    {:else if item.group}
      {#if i > 0}<div class="sep" role="separator"></div>{/if}
      <div class="group" role="presentation">{item.group}</div>
    {:else if item.separator && i > 0}<div class="sep" role="separator"></div>{/if}
    <button
      type="button"
      role="menuitem"
      class:active={i === active}
      class:danger={item.danger}
      class:off={item.disabled}
      class:why={item.reason}
      aria-disabled={item.disabled || item.reason ? 'true' : undefined}
      title={item.title ?? item.reason ?? undefined}
      onpointerenter={() => (active = item.disabled || item.reason ? -1 : i)}
      onclick={() => choose(i)}
    >
      {#if item.bar}<span class="bar" style:--bar={item.bar}></span>{:else}<span class="icon"
          >{#if item.icon}<Icon name={item.icon} size={16} />{/if}</span
        >{/if}
      {#if item.reason}
        <span class="text"
          ><span class="label">{item.label}</span><span class="reason">{item.reason}</span></span
        >
      {:else}
        <span class="label">{item.label}</span>
      {/if}
      {#if item.detail}<span class="detail">{item.detail}</span>{/if}
      {#if item.kbd}<Kbd chord={item.kbd} />{/if}
    </button>
  {/each}
  {#if footer}<div class="foot">{@render footer()}</div>{/if}
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
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: var(--k-space-2);
    border-radius: var(--k-radius-lg);
    background: var(--k-bg-float);
    box-shadow: var(--k-shadow);
    animation: k-pop-in var(--k-duration-fast) var(--k-ease);
  }

  .foot {
    margin-top: var(--k-space-2);
    padding: var(--k-space-2) var(--k-space-4) 0;
    border-top: 1px solid var(--k-border);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
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

  /* Not available now: the label muted, its reason under it. */
  button.why {
    height: auto;
    padding-block: var(--k-space-1);
    color: var(--k-fg-muted);
    cursor: default;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .reason {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
  }

  .group {
    padding: var(--k-space-1) var(--k-space-4);
    font-size: var(--k-font-size-xs);
    color: var(--k-fg-subtle);
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
    color: var(--k-fg-muted);
  }
</style>
