import { beforeEach, describe, expect, it } from 'vitest';

import { dispatch } from '$lib/actions';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { findContent } from '$lib/layout';
import { layout, projects, toasts, ui } from '$lib/stores';

import '../reviews/actions';
import '../tickets/actions';
import './actions';
import { workItem } from '$lib/gen/fixtures';

import { blockedReason } from './actions';
import { REVIEW_READONLY } from './common';
import { selectTicket } from './selection.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  ui.sheets = [];
  selectTicket(null, null);
  await projects.load();
  await layout.load('shop');
});

describe('L9 actions', () => {
  it('tickets.open opens (or focuses) the tickets pane of the active project', async () => {
    expect(await dispatch('tickets.open')).toBe(true);
    const l = layout.get('shop');
    const hit = l && findContent(l, (c) => c.kind === 'tickets');
    expect(hit).toBeTruthy();
    const tabs = l?.tabs.length;
    await dispatch('tickets.open');
    expect(layout.get('shop')?.tabs.length).toBe(tabs); // no second pane
  });

  it('reviews.open opens the reviews pane', async () => {
    expect(await dispatch('reviews.open')).toBe(true);
    const l = layout.get('shop');
    expect(l && findContent(l, (c) => c.kind === 'reviews')).toBeTruthy();
  });

  it('work.start without a selection explains what to do', async () => {
    await dispatch('work.start');
    expect(toasts.list.at(-1)?.toast.text).toMatch(/Select a ticket/);
    expect(mock.calls.some((c) => c.cmd === 'work_plan')).toBe(false);
  });

  it('work.start plans the selected ticket and opens the sheet', async () => {
    selectTicket({ account: 'jira-acme', key: 'SHOP-151', id: '10151' }, 'shop');
    await dispatch('work.start');
    expect(ui.sheet?.key).toBe('start_work');
    expect(mock.calls.filter((c) => c.cmd === 'work_plan').at(-1)?.args).toMatchObject({
      project_id: 'shop',
      source: { kind: 'ticket', ticket: { key: 'SHOP-151' } },
    });
  });

  it('work.start accepts an explicit ticket', async () => {
    await dispatch('work.start', {
      ticket: { account: 'redmine-corp', key: '4590', id: '4590' },
      project_id: 'billing',
    });
    expect(ui.sheet?.key).toBe('start_work');
  });

  it('a failing plan is reported with a toast', async () => {
    mock.failNext('work_plan', { code: 'invalid_argument', message: 'no repo rule matches' });
    selectTicket({ account: 'jira-acme', key: 'SHOP-151', id: '10151' }, 'shop');
    await dispatch('work.start');
    expect(ui.sheet).toBeNull();
    expect(toasts.list.at(-1)?.toast.text).toMatch(/no repo rule matches/);
  });

  it('review items: push, rebase, fix and link are read-only in the menu and the work bar', () => {
    const review = { ...workItem, kind: 'review' as const, pr_url: 'https://x/pull/1' };
    for (const id of ['ship', 'rebase', 'fix', 'link'] as const)
      expect(blockedReason(id, review)).toBe(REVIEW_READONLY);
    expect(blockedReason('go_claude', review)).toBeNull();
    expect(blockedReason('ship', { ...review, kind: 'ticket' })).toBeNull();
  });
});
