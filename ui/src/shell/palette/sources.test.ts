import { beforeEach, describe, expect, it } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { layout, plugins, projects, sessions, settings, tools, work } from '$lib/stores';

import '../actions';
import { rank } from './fuzzy';
import {
  actionItems,
  buildItems,
  GROUP_ORDER,
  itemText,
  projectItems,
  sessionItems,
  settingsItems,
  ticketItems,
  toolItems,
} from './sources';

beforeEach(async () => {
  setTransport(createMockTransport().transport);
  layout.byProject = {};
  await Promise.all([projects.load(), sessions.load(), work.load(), settings.load()]);
  await tools.load('shop');
  await plugins.load();
});

describe('palette sources', () => {
  it('lists the sessions of every project with their status, needing-input first', () => {
    const items = sessionItems();
    expect(items).toHaveLength(10);
    expect(items[0]!.detail).toContain('Needs input');
    expect(items[0]!.detail).toContain('Billing');
    expect(items.every((i) => i.group === 'Sessions')).toBe(true);
  });

  it('lists every configured project, open or not', () => {
    expect(projectItems().map((i) => i.label)).toEqual(['Home', 'Shop', 'Billing', 'Kelta tools']);
  });

  it('lists the registered actions with the effective chord of the platform', () => {
    const items = actionItems();
    const zoom = items.find((i) => i.id === 'action:pane.zoom')!;
    expect(zoom.label).toBe('Zoom pane');
    expect(zoom.kbd).toBeTruthy();
    // Actions owned by other lanes are not offered until they are registered; goto.N is omitted.
    expect(items.find((i) => i.id.startsWith('action:project.goto'))).toBeUndefined();
    expect(items.find((i) => i.id === 'action:tickets.open')).toBeUndefined();
  });

  it('lists tools of the active project and settings sections', () => {
    expect(toolItems().map((i) => i.label)).toContain('Open tool: lazygit');
    expect(settingsItems().map((i) => i.label)).toContain('Settings: Keys');
  });

  it('maps ticket search hits to palette items with their project', () => {
    const items = ticketItems([
      {
        ticket: {
          ref: { account: 'jira-acme', key: 'SHOP-1', id: '1' },
          title: 'Rate limit',
          url: 'https://x',
          status: { id: 's', name: 'In progress', category: 'in_progress' },
          kind: null,
          assignee: null,
          labels: [],
          priority: null,
          updated_at: '2026-10-01T00:00:00Z',
          project_hint: null,
        },
        project_ids: ['shop'],
        work_item_id: null,
      },
    ]);
    expect(items[0]).toMatchObject({ group: 'Tickets', label: 'SHOP-1 Rate limit' });
    expect(items[0]!.detail).toBe('In progress\u2002\u2002Shop');
  });

  it('ranks across groups and keeps the display group order', () => {
    const all = buildItems();
    expect(new Set(all.map((i) => i.group)).size).toBeGreaterThan(3);
    for (const g of GROUP_ORDER) expect(GROUP_ORDER.includes(g)).toBe(true);
    const hits = rank(all, 'shop claude', itemText);
    expect(hits[0]!.group).toBe('Sessions');
  });
});
