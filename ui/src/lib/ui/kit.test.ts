import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';

import Button from './Button.svelte';
import EmptyState from './EmptyState.svelte';
import ErrorState from './ErrorState.svelte';
import HtmlContent from './HtmlContent.svelte';
import Kbd from './Kbd.svelte';
import Menu from './Menu.svelte';
import NotImplemented from './NotImplemented.svelte';
import Tabs from './Tabs.svelte';
import Toggle from './Toggle.svelte';
import VirtualList from './VirtualList.svelte';
import { formatChord, relativeTime } from './format';
import { ICON_NAMES, injectSprite, spriteSvg } from './icons';
import VirtualListHarness from './VirtualListHarness.test.svelte';

describe('icons', () => {
  it('every icon name has a symbol in the sprite and vice versa', () => {
    const ids = [...spriteSvg.matchAll(/<symbol id="i-([a-z0-9-]+)"/g)].map((m) => m[1]);
    expect(ids.sort()).toEqual([...ICON_NAMES].sort());
    expect(ICON_NAMES.length).toBeGreaterThanOrEqual(60);
  });

  it('injects the sprite once', () => {
    injectSprite();
    injectSprite();
    expect(document.querySelectorAll('#kelta-sprite')).toHaveLength(1);
    expect(document.querySelector('#i-x')).not.toBeNull();
  });
});

describe('format', () => {
  it('formats chords per platform', () => {
    expect(formatChord('cmd+shift+k', 'macos')).toEqual(['⌘', '⇧', 'K']);
    expect(formatChord('mod+t', 'linux')).toEqual(['Ctrl', 'Shift', 'T']);
    expect(formatChord('ctrl+shift+pagedown', 'linux')).toEqual(['Ctrl', 'Shift', 'PgDn']);
    expect(formatChord('ctrl++', 'linux')).toEqual(['Ctrl', '+']);
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

  it('NotImplemented exposes its name', () => {
    render(NotImplemented, { props: { name: 'TicketsPane', owner: 'L9' } });
    expect(screen.getByTestId('not-implemented').dataset.name).toBe('TicketsPane');
  });

  it('Kbd shows chord parts', () => {
    const { container } = render(Kbd, { props: { chord: 'cmd+k', platform: 'macos' } });
    expect([...container.querySelectorAll('kbd')].map((k) => k.textContent)).toEqual(['⌘', 'K']);
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
});
