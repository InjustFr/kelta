// Settings search (#469): the ranking of the ticket's query table, what is indexed, and the results list.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { settingsSections } from '$app/registry';
import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { dispatch } from '$lib/actions';
import { projects, settings, ui } from '$lib/stores';
import { idle } from '$lib/stores/loadable';

import schemaJson from '../../../../schema/settings.schema.json';
import { effectiveBindings } from './lib/chords';
import type { LayerFile } from './lib/editor.svelte';
import type { SchemaNode } from './lib/schema';
import { buildSearchIndex, changedSettings, isInline, resettable, searchSettings } from './lib/search';
import './actions';
import SettingsPane from './SettingsPane.svelte';

const schema = schemaJson as unknown as SchemaNode;
beforeEach(() => vi.spyOn(navigator, 'platform', 'get').mockReturnValue('MacIntel'));
afterEach(() => vi.restoreAllMocks());

// built per test: the platform is read at build time
const index = () =>
  buildSearchIndex(
    schema,
    settingsSections.filter((s) => !s.platform || s.platform === 'macos'),
    effectiveBindings({}, 'macos'),
  );
const first = (q: string) => searchSettings(index(), q)[0]!;

const TABLE: [string, string | RegExp][] = [
  ['clipboard', /^key:terminal\./],
  ['do not disturb', 'key:notifications.quiet_hours'],
  ['dnd', 'key:notifications.quiet_hours'],
  ['quiet hours', 'key:notifications.quiet_hours'],
  ['branch name', 'key:worktree.branch_template'],
  ['worktree folder', 'key:worktree.root'],
  ['how often', 'key:polling.focused_secs'],
  ['refresh interval', 'key:polling.focused_secs'],
  ['pull request title', 'key:work.pr.title_template'],
  ['max sessions', 'key:claude.max_live'],
  ['concurrent', 'key:claude.max_live'],
  ['github token', 'row:add-account'],
  ['token', 'row:add-account'],
  ['shortcut', 'section:keys'],
  ['keybinding', 'section:keys'],
  ['hotkey', 'section:keys'],
  ['split', 'action:pane.split_right'],
  ['zoom', 'action:pane.zoom'],
  ['vscode', 'key:editor.default'],
  ['alt key', 'key:terminal.option_as_meta'],
  ['sound', 'key:terminal.bell'],
  ['add project', 'row:projects'],
  ['triggers', 'section:triggers'],
  ['plugins', 'section:plugins'],
  ['tags', 'section:tags'],
  ['permission mode', 'row:claude-permission-mode'],
  ['test notification', 'row:notification-test'],
];

describe('settings search ranking', () => {
  it.each(TABLE)('the table query %s puts the expected hit first', (q, expected) => {
    const id = first(q).id;
    if (typeof expected === 'string') expect(id).toBe(expected);
    else expect(id).toMatch(expected);
  });

  it('lazygit finds the Tools section first', () => expect(first('lazygit').section).toBe('tools'));

  it('zoom then font size; permission mode sits under Claude › Profiles', () => {
    expect(searchSettings(index(), 'zoom')[1]!.id).toBe('key:terminal.font_size');
    expect(first('permission mode').crumb).toBe('Claude › Profiles');
  });

  it('the refresh intervals sit under Sources › Refresh', () =>
    expect(first('how often')).toMatchObject({
      id: 'key:polling.focused_secs',
      section: 'sources',
      crumb: 'Sources › Refresh',
    }));

  it("'pr' matches whole words: a work.pr key first, no keys.prefix in the top 5", () => {
    const hits = searchSettings(index(), 'pr');
    expect(hits[0]!.id).toMatch(/^key:work\.pr\./);
    expect(
      hits
        .slice(0, 5)
        .map((h) => h.id)
        .join(),
    ).not.toMatch(/keys\.prefix/);
  });

  it('section names, TOML-only keys, custom rows and actions are hits', () => {
    expect(first('Work items')).toMatchObject({ kind: 'section', id: 'section:worktree' });
    expect(first('workflows')).toMatchObject({ id: 'key:workflows', toml: true, section: 'triggers' });
    expect(first('Send a test').id).toBe('row:notification-test');
    expect(first('Split down')).toMatchObject({ id: 'action:pane.split_down', chords: ['cmd+shift+d'] });
  });

  it('a hit says why it matched', () => {
    expect(first('clipboard').why).toBe('matches “clipboard”');
    const typo = first('notifcations');
    expect(typo.id).toBe('section:notifications');
    expect(typo.why).toBe('close match');
  });

  it('Advanced keys come after the rest', () => {
    const hits = searchSettings(index(), 'lines');
    const at = hits.findIndex((h) => h.id === 'key:terminal.view_scrollback');
    expect(at).toBeGreaterThan(-1);
    expect(hits.findLastIndex((h) => !h.advanced)).toBeLessThan(at);
  });
});

describe('settings search results', () => {
  it('lists and maps are never inline', () => {
    const lists = [
      'string-list',
      'string-map',
      'enum-map',
      'list-map',
      'object-map',
      'keyed-list',
      'json',
      'variant',
    ];
    for (const e of index()) if (e.toml || lists.includes(e.field ?? '')) expect(isInline(e)).toBe(false);
  });

  const pane = async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    await Promise.all([projects.load(), settings.load()]);
    render(SettingsPane, {
      projectId: 'shop',
      tabId: 't1',
      paneId: 'p1',
      visible: true,
      focused: true,
      content: { kind: 'settings', section: 'general' },
    });
    return screen.getByRole('searchbox', { name: 'Search settings' });
  };

  it('a list hit renders one line with Open, not its editor', async () => {
    const box = await pane();
    await fireEvent.input(box, { target: { value: 'prefix bindings' } });
    const li = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('[data-hit="key:keys.prefix_bindings"]');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(li.querySelector('[data-testid="field"]')).toBeNull();
    within(li).getByRole('button', { name: 'Open' });
  });

  it('settings.open { key } lands on the highlighted row in its section, also before the schema has loaded (#477)', async () => {
    settings.schema = idle();
    await dispatch('settings.open', { key: 'tickets.wip_limit' });
    const box = await pane();
    await waitFor(
      () =>
        expect(
          document
            .querySelector('[data-testid="field"][data-path="tickets.wip_limit"]')
            ?.hasAttribute('data-revealed'),
        ).toBe(true),
      { timeout: 3000 },
    );
    expect((box as HTMLInputElement).value).toBe('');
    expect(document.querySelector('nav [aria-current="page"]')?.getAttribute('data-section')).toBe(
      'worktree',
    );
    ui.settings = null;
  });

  it('nothing found offers ⌘K and the TOML file', async () => {
    const box = await pane();
    await fireEvent.input(box, { target: { value: 'zzqqxxkk' } });
    await screen.findByRole('button', { name: /Search actions in/ });
    await fireEvent.click(screen.getByRole('button', { name: 'Open the TOML file' }));
    expect(screen.getByTestId('edit-toml-toggle').textContent).toContain('Back to form');
  });
});

describe('Changed by you (#474)', () => {
  const files: LayerFile[] = [
    {
      layer: 'global',
      projectId: null,
      repoId: null,
      where: 'All projects',
      value: { terminal: { font_size: 15 }, keys: { bindings: { 'palette.open': ['cmd+p'] } }, sources: [] },
    },
    {
      layer: 'project',
      projectId: 'shop',
      repoId: null,
      where: 'Shop',
      value: { project: { id: 'shop' }, terminal: { font_size: 16 } },
    },
    {
      layer: 'repo',
      projectId: 'shop',
      repoId: 'api',
      where: 'Shop › api',
      value: { worktree: { setup: ['pnpm i'] } },
    },
  ];

  it('lists every key and rebound shortcut set in each file, with where it is set', () => {
    const rows = changedSettings(index(), files);
    expect(rows.map((r) => [r.file.where, r.kind, r.path])).toEqual(
      expect.arrayContaining([
        ['All projects', 'key', 'terminal.font_size'],
        ['All projects', 'key', 'sources'],
        ['All projects', 'action', 'palette.open'],
        ['Shop', 'key', 'terminal.font_size'],
        ['Shop › api', 'key', 'worktree.setup'],
      ]),
    );
    expect(rows).toHaveLength(5);
    expect(rows.some((r) => r.path === 'keys.bindings' || r.path?.startsWith('project'))).toBe(false);
    expect(rows.find((r) => r.path === 'palette.open')).toMatchObject({
      resets: ['keys.bindings."palette.open"'],
      chords: ['cmd+p'],
    });
    expect(rows.find((r) => r.file.where === 'Shop')!.value).toBe(16);
    expect(new Set(rows.map((r) => r.id)).size).toBe(rows.length);
  });

  it('offers reset for values and shortcuts, not for whole lists; the search narrows the rows', () => {
    const rows = changedSettings(index(), files);
    const row = (path: string) => rows.find((r) => r.path === path)!;
    expect(resettable(row('terminal.font_size'))).toBe(true);
    expect(resettable(row('palette.open'))).toBe(true);
    expect(resettable(row('sources'))).toBe(false);
    const hits = searchSettings(rows, 'font size');
    expect(hits.map((h) => h.file.where)).toEqual(['All projects', 'Shop']);
    expect(hits.every((h) => h.path === 'terminal.font_size')).toBe(true);
  });
});
