import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Tab } from '$lib/gen';
import { appInfo as sampleAppInfo } from '$lib/gen/fixtures';
import { createMockTransport } from '$lib/ipc/mock';
import { getTransport, setTransport } from '$lib/ipc/transport';
import { addTab, pane, updateTab } from '$lib/layout';
import { GLOBAL, layout, projects, sessions, tools, ui, work } from '$lib/stores';

import './actions';
import ContextBar from './ContextBar.svelte';
import { jumplist } from './jumplist';
import { activateProject, selectTab } from './nav';

// The native title call (core:window:allow-set-title); jsdom has no Tauri window.
const setTitle = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ setTitle }) }));

const SHOP_CLAUDE = '0199a6b2-0000-7000-8000-000000000001';
const LAZYDOCKER = '0199a6b2-0000-7000-8000-000000000004';

beforeEach(async () => {
  setTitle.mockClear();
  setTransport(createMockTransport().transport);
  ui.view = null;
  layout.byProject = {};
  work.byId = {};
  jumplist.list = [];
  jumplist.at = -1;
  await Promise.all([projects.load(), sessions.load(), work.load()]);
  await Promise.all(['shop', 'home'].map((p) => layout.ensure(p)));
  await tools.load('shop');
  await activateProject('shop');
});

/** The breadcrumb's text: the glyph is spaced by CSS, not by text. */
const crumb = () => screen.getByTestId('context-crumb').textContent?.replace(/\s+/g, ' ').trim();
const bar = () => screen.getByTestId('contextbar');

/** A tab of `projectId` holding only the lazydocker tool session. */
function toolTab(projectId: string): void {
  const tab: Tab = {
    id: 'tool-tab',
    title: 'lazydocker',
    work_item_id: null,
    root: pane({ kind: 'terminal', session_id: LAZYDOCKER }, 'tool-pane'),
    focused_pane: 'tool-pane',
    zoomed_pane: null,
  };
  layout.update(projectId, (l) => addTab(l, tab));
}

describe('context bar', () => {
  it('work item: the key and title, then the work segment; no cwd, nothing from the pane header', async () => {
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Shop › SHOP-142 Rate-limit login attempts'));
    expect(await screen.findByTestId('context-work')).toBeTruthy();
    expect(screen.getByTestId('work-menu-button')).toBeTruthy();
    expect(screen.queryByTestId('context-meta')).toBeNull();
    expect(bar().textContent).not.toMatch(/Live status off|ctx|Claude \d/);
    // Native decorations: the window title follows the breadcrumb.
    expect(document.title).toBe('Shop › SHOP-142 Rate-limit login attempts');
    await waitFor(() =>
      expect(setTitle).toHaveBeenLastCalledWith('Shop › SHOP-142 Rate-limit login attempts'),
    );
  });

  it('plain terminal: ❯ its OSC 2 title and the cwd, nothing more', async () => {
    selectTab('shop', 'shop-t2');
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Shop › ❯~/code/shop-api'));
    expect(screen.getByTestId('context-meta').textContent).toBe('~/code/shop-api');
    expect(screen.queryByTestId('context-work')).toBeNull();
    expect(document.title).toBe('Shop › ~/code/shop-api');
  });

  it('Claude without a work item: ✻ its name, the cwd and the model', async () => {
    const s = sessions.get(SHOP_CLAUDE)!;
    sessions.upsert({ ...s, work_item_id: null });
    layout.update('shop', (l) => updateTab(l, 'shop-t1', (t) => ({ ...t, work_item_id: null })));
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Shop › ✻claude'));
    expect(screen.getByTestId('context-meta').textContent).toBe(
      '~/code/shop-api.worktrees/SHOP-142·claude-sonnet-5',
    );
    expect(screen.queryByTestId('context-work')).toBeNull();
  });

  it('tool without a project argument: Tools › tool, "for you", and the return chip is nav.back', async () => {
    toolTab('home');
    // Opened from Now: the jumplist holds Now, then the tool.
    jumplist.list = [
      { project: GLOBAL, tab: 'now', pane: 'now' },
      { project: 'home', tab: 'tool-tab', pane: 'tool-pane' },
    ];
    jumplist.at = 1;
    await activateProject('home');
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Tools › ⚒lazydocker'));
    expect(bar().textContent).toContain('for you');
    expect(screen.queryByTestId('context-owner')).toBeNull();
    const chip = screen.getByTestId('context-return');
    expect(chip.textContent).toContain('↩ Now');
    await fireEvent.click(chip);
    await waitFor(() => expect(ui.view).toBe('now'));
  });

  it('tool in a project: the context chip goes to that project, then the cwd', async () => {
    toolTab('shop');
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Tools › ⚒lazydocker'));
    expect(bar().textContent).not.toContain('for you');
    expect(screen.getByText('~/code/shop-api')).toBeTruthy();
    await fireEvent.click(screen.getByTestId('context-owner'));
    expect(layout.get('shop')?.active_tab).toBe('shop-t1');
  });

  it('Home: Home › the terminal and its cwd', async () => {
    await activateProject('home');
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Home › ❯~'));
    expect(screen.getByTestId('context-meta').textContent).toBe('~');
  });

  it('global view: its name, and the "underneath" chip leaves the view', async () => {
    ui.view = 'now';
    render(ContextBar);
    await waitFor(() => expect(crumb()).toBe('Now'));
    await waitFor(() => expect(setTitle).toHaveBeenLastCalledWith('Now'));
    const under = screen.getByTestId('context-underneath');
    expect(under.textContent).toContain('‹ Shop › SHOP-142 Rate-limit login');
    expect(screen.queryByTestId('context-work')).toBeNull();
    await fireEvent.click(under);
    expect(ui.view).toBeNull();
    expect(projects.activeId).toBe('shop');
  });

  it('no project and no view: the native window title is Kelta', async () => {
    projects.list = projects.list.map((p) => ({ ...p, active: false }));
    render(ContextBar);
    await waitFor(() => expect(setTitle).toHaveBeenLastCalledWith('Kelta'));
  });

  it('‹ names the jumplist target', async () => {
    jumplist.list = [{ project: GLOBAL, tab: 'reviews', pane: 'reviews' }];
    jumplist.at = 0;
    render(ContextBar);
    await waitFor(() =>
      expect(screen.getByTestId('context-back').getAttribute('aria-label')).toBe('Back to Reviews'),
    );
    expect(screen.getByTestId('tab-plus')).toBeTruthy();
  });
});

describe('context bar as the title bar (custom decorations)', () => {
  const realPlatform = Object.getOwnPropertyDescriptor(Navigator.prototype, 'platform');

  function customChrome(platform: string): void {
    Object.defineProperty(navigator, 'platform', { value: platform, configurable: true });
    const base = getTransport();
    setTransport({
      ...base,
      invoke: <T>(cmd: string, args?: Record<string, unknown> | Uint8Array) =>
        cmd === 'app_info'
          ? Promise.resolve({ ...sampleAppInfo, decorations: 'custom' } as T)
          : base.invoke<T>(cmd, args),
    });
  }

  afterEach(() => {
    delete (navigator as { platform?: string }).platform;
    if (realPlatform) Object.defineProperty(Navigator.prototype, 'platform', realPlatform);
  });

  it('macOS: one bar with the traffic-light inset and a drag region, no Kelta window buttons', async () => {
    customChrome('MacIntel');
    render(ContextBar);
    await waitFor(() => expect(bar().dataset.custom).toBe('true'));
    expect(bar().classList.contains('inset')).toBe(true);
    expect(bar().hasAttribute('data-tauri-drag-region')).toBe(true);
    expect(screen.getByTestId('context-crumb').hasAttribute('data-tauri-drag-region')).toBe(true);
    expect(screen.queryByRole('button', { name: 'Close window' })).toBeNull();
    expect(screen.queryByTestId('titlebar')).toBeNull();
  });

  it('Linux: the window buttons sit after `+`, no inset', async () => {
    customChrome('Linux x86_64');
    render(ContextBar);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Close window' })).toBeTruthy());
    expect(bar().classList.contains('inset')).toBe(false);
    expect(
      screen
        .getByTestId('tab-plus')
        .compareDocumentPosition(screen.getByRole('button', { name: 'Minimize' })),
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });
});
