import { describe, expect, it } from 'vitest';

import { paneComponent, paneRegistry, settingsSections, sheetRegistry, tabHeaderRegistry } from './registry';

// The first import compiles every lazy view: far over vitest's 5 s default on a loaded machine.
describe('registries', { timeout: 60_000 }, () => {
  it('every pane kind except empty has a lazy view', async () => {
    const kinds = Object.keys(paneRegistry).sort();
    expect(kinds).toEqual(
      [
        'diagnostics',
        'inbox',
        'plugin_screen',
        'review_detail',
        'reviews',
        'settings',
        'terminal',
        'ticket_detail',
        'tickets',
        'web',
        'welcome',
        'work_item',
      ].sort(),
    );
    expect(paneComponent('empty')).toBeNull();
    for (const load of Object.values(paneRegistry)) {
      expect(typeof (await load()).default).toBe('function');
    }
  });

  it('sheets and tab headers load', async () => {
    expect(Object.keys(sheetRegistry).sort()).toEqual(
      ['onboarding', 'plugin_install', 'project_new', 'start_work', 'tool_picker'].sort(),
    );
    for (const load of [...Object.values(sheetRegistry), ...Object.values(tabHeaderRegistry)]) {
      expect(typeof (await load()).default).toBe('function');
    }
  });

  it('settings sections are unique, ordered per BUILD_PLAN §2.4 and load', async () => {
    const ids = settingsSections.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).toEqual([
      'general',
      'window',
      'keys',
      'terminal',
      'linux-graphics',
      'projects',
      'accounts',
      'claude',
      'editors',
      'worktree',
      'reviews',
      'polling',
      'notifications',
      'performance',
      'tools',
      'triggers',
      'plugins',
    ]);
    for (const s of settingsSections) expect(typeof (await s.load()).default).toBe('function');
  });
});
