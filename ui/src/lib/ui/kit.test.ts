import { fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import Button from './Button.svelte';
import EmptyState from './EmptyState.svelte';
import ErrorState from './ErrorState.svelte';
import HtmlContent from './HtmlContent.svelte';
import IconButton from './IconButton.svelte';
import Kbd from './Kbd.svelte';
import Lamp from './Lamp.svelte';
import Menu from './Menu.svelte';
import Tabs from './Tabs.svelte';
import Toggle from './Toggle.svelte';
import VirtualList from './VirtualList.svelte';
import { formatChord, relativeTime } from './format';
import { ICON_NAMES, ensureIcon, injectSprite, isIconName, spriteSvg } from './icons';
import VirtualListHarness from './VirtualListHarness.test.svelte';

describe('icons', () => {
  it('names every Lucide icon and builds the sprite from them', () => {
    expect(ICON_NAMES.length).toBeGreaterThan(1500);
    expect(isIconName('container')).toBe(true);
    expect(isIconName('not-an-icon')).toBe(false);
    const ids = [...spriteSvg.matchAll(/<symbol id="i-([a-z0-9-]+)" viewBox="0 0 24 24"><path /g)].map(
      (m) => m[1],
    );
    expect(ids).toContain('x');
    expect(ids.every((id) => isIconName(id))).toBe(true);
  });

  it('injects the sprite once', () => {
    injectSprite();
    injectSprite();
    expect(document.querySelectorAll('#kelta-sprite')).toHaveLength(1);
    expect(document.querySelector('#i-x')).not.toBeNull();
  });

  it('loads an icon outside the sprite on first use', async () => {
    injectSprite();
    expect(ensureIcon('x')).toBeUndefined();
    expect(ensureIcon('not-an-icon')).toBeUndefined();
    expect(document.querySelector('#i-a-arrow-down')).toBeNull();
    const load = ensureIcon('a-arrow-down');
    expect(ensureIcon('a-arrow-down')).toBe(load);
    // A failed load leaves the icon blank and the next use retries (that is the app's contract); retry the
    // same way, so one failure of the chunk under load (#540) is not the test's verdict.
    await vi.waitFor(
      async () => {
        await ensureIcon('a-arrow-down');
        expect(document.querySelectorAll('#kelta-sprite svg > #i-a-arrow-down path').length).toBeGreaterThan(
          0,
        );
      },
      { timeout: 10_000 },
    );
    expect(ensureIcon('a-arrow-down')).toBeUndefined();
  });
});

describe('format', () => {
  it('formats chords per platform', () => {
    expect(formatChord('cmd+shift+k', 'macos')).toEqual(['⌘', '⇧', 'K']);
    expect(formatChord('mod+t', 'linux')).toEqual(['Ctrl', 'T']);
    expect(formatChord('ctrl+shift+pagedown', 'linux')).toEqual(['Ctrl', 'Shift', 'PgDn']);
    expect(formatChord('ctrl++', 'linux')).toEqual(['Ctrl', '+']);
    expect(formatChord('V', 'macos')).toEqual(['⇧', 'V']);
    expect(formatChord('v', 'macos')).toEqual(['V']);
  });

  it('relative times', () => {
    expect(relativeTime(0, 10_000)).toBe('just now');
    expect(relativeTime(0, 5 * 60_000)).toBe('5 min ago');
    expect(relativeTime(0, 3 * 3600_000)).toBe('3 h ago');
  });
});

describe('components', () => {
  it('Button renders and clicks; loading disables', async () => {
    const onclick = vi.fn();
    const { rerender } = render(Button, { props: { onclick, icon: 'plus' } });
    await fireEvent.click(screen.getByRole('button'));
    expect(onclick).toHaveBeenCalledOnce();
    await rerender({ onclick, loading: true });
    expect(screen.getByRole('button')).toHaveProperty('disabled', true);
  });

  it('EmptyState and ErrorState', async () => {
    render(EmptyState, { props: { title: 'No review requests.' } });
    expect(screen.getByText('No review requests.')).toBeTruthy();
    const onretry = vi.fn();
    render(ErrorState, {
      props: { error: { code: 'network', message: 'offline', detail: null, retry_after_ms: null }, onretry },
    });
    expect(screen.getByRole('alert').textContent).toContain('offline');
    await fireEvent.click(screen.getByText('Retry'));
    expect(onretry).toHaveBeenCalledOnce();
  });

  it('Kbd shows one mark per chord', () => {
    const caps = (platform: 'macos' | 'linux') =>
      [...render(Kbd, { props: { chord: 'mod+k', platform } }).container.querySelectorAll('kbd')].map(
        (k) => k.textContent,
      );
    expect(caps('macos')).toEqual(['⌘K']);
    expect(caps('linux')).toEqual(['Ctrl+K']);
  });

  it('Tabs selects with click and arrows', async () => {
    const onchange = vi.fn();
    render(Tabs, {
      props: {
        items: [
          { id: 'list', label: 'List' },
          { id: 'board', label: 'Board' },
        ],
        value: 'list',
        onchange,
      },
    });
    await fireEvent.click(screen.getByText('Board'));
    expect(onchange).toHaveBeenLastCalledWith('board');
    await fireEvent.keyDown(screen.getByRole('tablist'), { key: 'ArrowRight' });
    expect(onchange).toHaveBeenLastCalledWith('list');
  });

  it('Tabs with no selected tab keeps the first enabled one in the Tab order', () => {
    render(Tabs, {
      props: {
        items: [
          { id: 'a', label: 'A', disabled: true },
          { id: 'b', label: 'B' },
        ],
      },
    });
    expect(screen.getAllByRole('tab').map((t) => t.tabIndex)).toEqual([-1, 0]);
  });

  it('Toggle reports changes', async () => {
    const onchange = vi.fn();
    render(Toggle, { props: { label: 'Drafts', onchange } });
    await fireEvent.click(screen.getByRole('switch'));
    expect(onchange).toHaveBeenCalledWith(true);
  });

  it('Menu navigates with the keyboard and selects', async () => {
    const onselect = vi.fn();
    const onclose = vi.fn();
    render(Menu, {
      props: {
        x: 10,
        y: 10,
        onselect,
        onclose,
        items: [
          { id: 'a', label: 'A' },
          { id: 'b', label: 'B', disabled: true },
          { id: 'c', label: 'C', danger: true },
        ],
      },
    });
    const menu = screen.getByRole('menu');
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'Enter' });
    expect(onselect).toHaveBeenCalledWith('c');
    expect(onclose).toHaveBeenCalled();
  });

  it('Menu runs an item by its key; a disabled one swallows it; Enter runs the first', async () => {
    const onselect = vi.fn();
    render(Menu, {
      props: {
        x: 10,
        y: 10,
        onselect,
        onclose: vi.fn(),
        items: [
          { id: 'a', label: 'A' },
          { id: 'b', label: 'B', key: 'b', disabled: true, title: 'not available yet' },
          { id: 'f', label: 'F', key: 'F' },
        ],
      },
    });
    const menu = screen.getByRole('menu');
    await fireEvent.keyDown(menu, { key: 'b' });
    expect(onselect).not.toHaveBeenCalled();
    expect(screen.getByText('B').closest('button')?.title).toBe('not available yet');
    await fireEvent.keyDown(menu, { key: 'F' });
    expect(onselect).toHaveBeenCalledWith('f');
    await fireEvent.keyDown(menu, { key: 'Enter' });
    expect(onselect).toHaveBeenLastCalledWith('a');
  });

  it('HtmlContent routes links to open_external and never navigates', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    render(HtmlContent, {
      props: {
        html: '<p>See <a href="https://example.com/x">x</a> and <a href="javascript:alert(1)">y</a></p>',
      },
    });
    const good = screen.getByText('x');
    const ev = new MouseEvent('click', { bubbles: true, cancelable: true });
    good.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    await fireEvent.click(screen.getByText('y'));
    await Promise.resolve();
    const opened = controls.calls.filter((c) => c.cmd === 'open_external').map((c) => c.args);
    expect(opened).toEqual([{ url: 'https://example.com/x' }]);
  });

  it('VirtualList renders only a window of rows', () => {
    const items = Array.from({ length: 1000 }, (_, i) => `row ${i}`);
    const { container } = render(VirtualListHarness, { props: { items } });
    const rows = container.querySelectorAll('[role="listitem"]');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(60);
    expect(container.querySelector('.spacer')?.getAttribute('style')).toContain('28000px');
    void VirtualList;
  });

  it('VirtualList with measure places rows after their measured heights', async () => {
    let report: ResizeObserverCallback = () => {};
    const observed: HTMLElement[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: ResizeObserverCallback) {
          report = cb;
        }
        observe(el: HTMLElement) {
          observed.push(el);
        }
        unobserve() {}
        disconnect() {}
      },
    );
    const items = Array.from({ length: 100 }, (_, i) => `row ${i}`);
    const { container } = render(VirtualListHarness, { props: { items, measure: true } });
    const second = observed.find((el) => el.textContent === 'row 1')!;
    Object.defineProperty(second, 'offsetHeight', { value: 50 });
    report(
      observed.map((target) => ({ target }) as unknown as ResizeObserverEntry),
      {} as ResizeObserver,
    );
    await Promise.resolve();
    expect(container.querySelector('.spacer')?.getAttribute('style')).toContain(`${99 * 28 + 50}px`);
    const third = [...container.querySelectorAll<HTMLElement>('[role="listitem"]')].find(
      (el) => el.textContent === 'row 2',
    );
    expect(third?.style.top).toBe('78px');
    vi.unstubAllGlobals();
  });

  it('VirtualList measures rows already mounted when measure turns on', async () => {
    let report: ResizeObserverCallback = () => {};
    const observed: HTMLElement[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: ResizeObserverCallback) {
          report = cb;
        }
        observe(el: HTMLElement) {
          observed.push(el);
        }
        unobserve() {}
        disconnect() {}
      },
    );
    const items = Array.from({ length: 100 }, (_, i) => `row ${i}`);
    const { container, rerender } = render(VirtualListHarness, { props: { items, measure: false } });
    await rerender({ items, measure: true });
    const second = observed.find((el) => el.textContent === 'row 1')!;
    Object.defineProperty(second, 'offsetHeight', { value: 50 });
    report([{ target: second } as unknown as ResizeObserverEntry], {} as ResizeObserver);
    await Promise.resolve();
    const third = [...container.querySelectorAll<HTMLElement>('[role="listitem"]')].find(
      (el) => el.textContent === 'row 2',
    );
    expect(third?.style.top).toBe('78px');
    vi.unstubAllGlobals();
  });

  it('VirtualList scrollToIndex callers are not re-run by a measured size change', async () => {
    let report: ResizeObserverCallback = () => {};
    const observed: HTMLElement[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: ResizeObserverCallback) {
          report = cb;
        }
        observe(el: HTMLElement) {
          observed.push(el);
        }
        unobserve() {}
        disconnect() {}
      },
    );
    const items = Array.from({ length: 100 }, (_, i) => `row ${i}`);
    const { container } = render(VirtualListHarness, { props: { items, measure: true, follow: 0 } });
    const viewport = container.querySelector<HTMLElement>('.k-vlist')!;
    viewport.scrollTop = 500;
    viewport.dispatchEvent(new Event('wheel'));
    const row = observed.find((el) => el.textContent === 'row 1')!;
    Object.defineProperty(row, 'offsetHeight', { value: 50 });
    report([{ target: row } as unknown as ResizeObserverEntry], {} as ResizeObserver);
    await Promise.resolve();
    await Promise.resolve();
    expect(viewport.scrollTop).toBe(500);
    vi.unstubAllGlobals();
  });

  it('VirtualList keeps a far followed row in view while rows measure', async () => {
    let report: ResizeObserverCallback = () => {};
    const observed = new Set<HTMLElement>();
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: ResizeObserverCallback) {
          report = cb;
        }
        observe(el: HTMLElement) {
          observed.add(el);
        }
        unobserve() {}
        disconnect() {}
      },
    );
    const items = Array.from({ length: 100 }, (_, i) => `row ${i}`);
    const { container } = render(VirtualListHarness, { props: { items, measure: true, follow: 40 } });
    const viewport = container.querySelector<HTMLElement>('.k-vlist')!;
    Object.defineProperty(viewport, 'clientHeight', { value: 300 });
    await Promise.resolve();
    for (const el of observed) Object.defineProperty(el, 'offsetHeight', { value: 54 });
    report(
      [...observed].map((target) => ({ target }) as unknown as ResizeObserverEntry),
      {} as ResizeObserver,
    );
    await Promise.resolve();
    // Row 40 is measured by now (rendered around the new scroll position) or still estimated; either way it must be on screen.
    const row = [...container.querySelectorAll<HTMLElement>('[role="listitem"]')].find(
      (el) => el.textContent === 'row 40',
    )!;
    const y = parseFloat(row.style.top);
    expect(y).toBeGreaterThanOrEqual(viewport.scrollTop);
    expect(y).toBeLessThan(viewport.scrollTop + 300);
    vi.unstubAllGlobals();
  });

  it('VirtualList pins the header of the group in view, pushed up by the next one', async () => {
    const items = Array.from({ length: 100 }, (_, i) => (i % 10 === 0 ? `head ${i}` : `row ${i}`));
    const { container } = render(VirtualListHarness, {
      props: { items, sticky: (s) => s.startsWith('head') },
    });
    const list = container.querySelector<HTMLElement>('.k-vlist')!;
    const pin = () => container.querySelector<HTMLElement>('.pin');
    const scroll = async (top: number) => {
      Object.defineProperty(list, 'scrollTop', { value: top, configurable: true });
      await fireEvent.scroll(list);
    };
    expect(pin()).toBeNull(); // the first header is in the list itself
    await scroll(28 * 12);
    expect(pin()?.textContent).toBe('head 10');
    expect(pin()?.style.translate).toBe('0 0px');
    await scroll(28 * 19 + 10); // head 20 is 18px below the top: head 10 slides up 10px
    expect(pin()?.textContent).toBe('head 10');
    expect(pin()?.style.translate).toBe('0 -10px');
  });

  it('VirtualList scrolls a row clear of the pinned header', () => {
    const items = Array.from({ length: 100 }, (_, i) => (i % 10 === 0 ? `head ${i}` : `row ${i}`));
    const { container, component } = render(VirtualListHarness, {
      props: { items, sticky: (s) => s.startsWith('head') },
    });
    const vp = container.querySelector<HTMLElement>('.k-vlist')!;
    vp.scrollTop = 28 * 15;
    component.scrollToIndex(12);
    expect(vp.scrollTop).toBe(28 * 11);
  });
});

describe('labelled primitives', () => {
  const textSnippet = (t: string) => createRawSnippet(() => ({ render: () => `<span>${t}</span>` }));

  it('Button chord is decorative and keeps the accessible name', () => {
    render(Button, { props: { chord: 'cmd+k', children: textSnippet('Search') } });
    const btn = screen.getByRole('button', { name: 'Search' });
    expect(btn.querySelector('.chord')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('IconButton puts the chord in the tooltip only', () => {
    render(IconButton, { props: { icon: 'x', label: 'Close pane', chord: 'ctrl+w' } });
    const btn = screen.getByRole('button', { name: 'Close pane' });
    expect(btn.getAttribute('title')).toMatch(/^Close pane \(.+\)$/);
  });

  it('Lamp with a label shows the word and hides the lamp from the tree', () => {
    const { container } = render(Lamp, { props: { level: 'needs_input', label: 'Needs input' } });
    expect(screen.getByText('Needs input')).toBeTruthy();
    expect(container.querySelector('.lamp')?.getAttribute('aria-hidden')).toBe('true');
    expect(screen.queryByRole('img')).toBeNull();
  });
});
