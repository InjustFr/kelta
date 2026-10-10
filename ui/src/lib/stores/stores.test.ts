import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { UiEvent } from '$lib/gen';
import { connectUiEvents, onAnyUiEvent, resetUiEventsForTests } from '$lib/ipc/events';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { makeTab, pane } from '$lib/layout';

import { LayoutStore, LAYOUT_SAVE_DEBOUNCE_MS } from './layout.svelte';
import { PluginsStore } from './plugins.svelte';
import { ProjectsStore } from './projects.svelte';
import { ReviewsStore } from './reviews.svelte';
import { SessionsStore } from './sessions.svelte';
import { SettingsStore } from './settings.svelte';
import { TicketsStore } from './tickets.svelte';
import { ToastsStore } from './toasts.svelte';
import { UiStore } from './ui.svelte';
import { WorkStore } from './work.svelte';

let mock: MockControls;

beforeEach(() => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  resetUiEventsForTests();
});

afterEach(() => {
  vi.useRealTimers();
});

const flush = async (): Promise<void> => {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
};

describe('ProjectsStore', () => {
  it('loads and activates projects', async () => {
    const store = new ProjectsStore();
    await store.load();
    expect(store.list.length).toBe(4);
    expect(store.activeId).toBe('shop');
    expect(store.home?.builtin).toBe(true);
    await store.activate('billing');
    expect(store.activeId).toBe('billing');
    expect(store.list.filter((p) => p.active)).toHaveLength(1);
  });

  it('rolls back an optimistic activation on error', async () => {
    const store = new ProjectsStore();
    await store.load();
    mock.failNext('project_activate', { code: 'internal' });
    await expect(store.activate('billing')).rejects.toMatchObject({ code: 'internal' });
    expect(store.activeId).toBe('shop');
  });

  it('records load errors', async () => {
    const store = new ProjectsStore();
    mock.failNext('project_list', { code: 'unsupported', message: 'not implemented: project_list' });
    await expect(store.load()).rejects.toBeTruthy();
    expect(store.error?.code).toBe('unsupported');
  });

  it('reorders locally and persists', async () => {
    const store = new ProjectsStore();
    await store.load();
    const ids = store.list.map((p) => p.id).reverse();
    await store.reorder(ids);
    expect(store.list.map((p) => p.id)).toEqual(ids);
    expect(mock.calls.at(-1)).toMatchObject({ cmd: 'project_reorder' });
  });
});

describe('SessionsStore', () => {
  it('loads all sessions and filters by project', async () => {
    const store = new SessionsStore();
    await store.load();
    expect(store.all).toHaveLength(11);
    expect(store.forProject('shop')).toHaveLength(5);
    expect(store.needingInput.map((s) => s.project_id)).toEqual(['billing']);
    expect(store.next(['shop', 'billing'], null)?.project_id).toBe('billing');
  });

  it('follows session events', async () => {
    const store = new SessionsStore();
    await store.load();
    const first = store.all[0]!;
    store.apply({ type: 'session.updated', session: { ...first, name: 'renamed' } });
    expect(store.get(first.id)?.name).toBe('renamed');
    store.apply({ type: 'session.removed', id: first.id });
    expect(store.get(first.id)).toBeNull();
  });

  it('marks a session seen optimistically', async () => {
    const store = new SessionsStore();
    await store.load();
    const unseen = store.all.find((s) => !s.seen)!;
    await store.markSeen(unseen.id);
    expect(store.get(unseen.id)?.seen).toBe(true);
  });
});

describe('LayoutStore', () => {
  it('debounces saves and updates rev', async () => {
    vi.useFakeTimers();
    const store = new LayoutStore();
    const loaded = await store.load('shop');
    store.update('shop', (l) => ({ ...l, active_tab: l.tabs[1]!.id }));
    store.update('shop', (l) => ({ ...l, active_tab: l.tabs[2]!.id }));
    expect(store.isDirty('shop')).toBe(true);
    expect(mock.calls.filter((c) => c.cmd === 'layout_save')).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(LAYOUT_SAVE_DEBOUNCE_MS + 1);
    await flush();
    expect(mock.calls.filter((c) => c.cmd === 'layout_save')).toHaveLength(1);
    expect(store.get('shop')?.rev).toBe(loaded.rev + 1);
    expect(store.isDirty('shop')).toBe(false);
  });

  it('refetches on a rev conflict', async () => {
    const store = new LayoutStore();
    await store.load('shop');
    mock.state.layouts.shop = { ...mock.state.layouts.shop!, rev: 99 };
    store.update('shop', (l) => ({ ...l, active_tab: l.tabs[1]!.id }));
    await store.flush('shop');
    expect(store.get('shop')?.rev).toBe(99);
  });

  it('reports non-conflict save errors', async () => {
    const store = new LayoutStore();
    const onSaveError = vi.fn();
    store.onSaveError = onSaveError;
    await store.load('shop');
    mock.failNext('layout_save', { code: 'internal' });
    store.update('shop', (l) => ({ ...l, active_tab: l.tabs[1]!.id }));
    await store.flush('shop');
    expect(onSaveError).toHaveBeenCalledOnce();
  });

  it('opens panes and backend changes win', async () => {
    vi.useFakeTimers();
    const store = new LayoutStore();
    await store.load('home');
    const loc = store.open('home', {
      content: { kind: 'diagnostics' },
      placement: 'new_tab',
      focus: true,
      tab_title: null,
      work_item_id: null,
    });
    expect(loc).not.toBeNull();
    expect(store.isDirty('home')).toBe(true);
    const tab = makeTab('X', pane({ kind: 'welcome' }));
    store.apply({
      type: 'layout.changed',
      project_id: 'home',
      layout: { project_id: 'home', tabs: [tab], active_tab: tab.id, rev: 50 },
    });
    expect(store.isDirty('home')).toBe(false);
    expect(store.get('home')?.rev).toBe(50);
  });

  it('applies ui.open requests and schedules a save', async () => {
    vi.useFakeTimers();
    const store = new LayoutStore();
    await store.load('home');
    store.apply({
      type: 'ui.open',
      project_id: 'home',
      request: {
        content: { kind: 'inbox' },
        placement: 'focused',
        focus: true,
        tab_title: null,
        work_item_id: null,
      },
    });
    // The Inbox tab already exists: it is focused, and the change is saved.
    expect(store.get('home')?.active_tab).toBe('home-t2');
    expect(store.isDirty('home')).toBe(true);
  });
});

describe('TicketsStore', () => {
  it('loads lists per scope and view', async () => {
    const store = new TicketsStore();
    await store.load({ kind: 'project', id: 'shop' }, 'mine');
    expect(store.items({ kind: 'project', id: 'shop' }, 'mine').length).toBeGreaterThan(0);
    await store.load({ kind: 'all' });
    expect(store.items({ kind: 'all' }).some((t) => t.project_ids.length === 0)).toBe(true);
  });

  it('moves optimistically and rolls back on error', async () => {
    const store = new TicketsStore();
    const scope = { kind: 'project' as const, id: 'shop' };
    await store.load(scope, 'mine');
    const cols = (await store.loadColumns('shop')).data!;
    const ticket = store.items(scope, 'mine').find((t) => t.ticket.status.category === 'todo')!.ticket;
    const done = cols.find((c) => c.category === 'in_review')!;

    mock.failNext('tracker_move', { code: 'conflict', message: 'ambiguous' });
    const pending = store.move(ticket, done);
    expect(
      store.items(scope, 'mine').find((t) => t.ticket.ref.key === ticket.ref.key)?.ticket.status.category,
    ).toBe('in_review');
    await expect(pending).rejects.toMatchObject({ code: 'conflict' });
    expect(
      store.items(scope, 'mine').find((t) => t.ticket.ref.key === ticket.ref.key)?.ticket.status,
    ).toEqual(ticket.status);

    const moved = await store.move(ticket, done);
    expect(moved.status.category).toBe('in_review');
  });

  it('refetches loaded lists on tickets.changed', async () => {
    const store = new TicketsStore();
    const scope = { kind: 'project' as const, id: 'billing' };
    await store.load(scope, 'mine');
    const before = mock.calls.filter((c) => c.cmd === 'tracker_list').length;
    store.apply({ type: 'tickets.changed', scope: { kind: 'project', id: 'shop' } });
    await flush();
    expect(mock.calls.filter((c) => c.cmd === 'tracker_list').length).toBe(before);
    store.apply({ type: 'tickets.changed', scope });
    await flush();
    await flush();
    expect(mock.calls.filter((c) => c.cmd === 'tracker_list').length).toBe(before + 1);
  });

  it('keeps stale data when a refresh fails', async () => {
    const store = new TicketsStore();
    await store.load({ kind: 'all' });
    mock.failNext('tracker_list', { code: 'network', message: 'offline' });
    const l = await store.load({ kind: 'all' }, null, true);
    expect(l.error?.code).toBe('network');
    expect(l.stale).toBe(true);
    expect(l.data?.items.length).toBeGreaterThan(0);
  });

  it('loads details and transitions', async () => {
    const store = new TicketsStore();
    const ref = { account: 'jira-acme', key: 'SHOP-142', id: '10142' };
    expect((await store.loadDetail(ref)).data?.ticket.ref.key).toBe('SHOP-142');
    expect((await store.loadTransitions(ref)).data?.length).toBeGreaterThan(0);
  });
});

describe('ReviewsStore', () => {
  it('loads by kind and tracks new keys', async () => {
    const store = new ReviewsStore();
    const l = await store.load({ kind: 'all' }, 'review_requested');
    expect(l.data?.items.every((r) => r.review.kind === 'review_requested')).toBe(true);
    const ref = l.data!.items[0]!.review.ref;
    store.apply({ type: 'reviews.changed', scope: { kind: 'all' }, new_keys: [ref] });
    expect(store.isNew(ref)).toBe(true);
    store.markSeen(ref);
    expect(store.isNew(ref)).toBe(false);
  });

  it('approve with a stale head sha conflicts', async () => {
    const store = new ReviewsStore();
    const l = await store.load({ kind: 'all' }, 'review_requested');
    const review = l.data!.items[0]!.review;
    const { reviewApprove } = await import('$lib/ipc/commands');
    await expect(reviewApprove({ review: review.ref, head_sha: 'old' })).rejects.toMatchObject({
      code: 'conflict',
    });
    await expect(reviewApprove({ review: review.ref, head_sha: review.head_sha })).resolves.toBeNull();
  });
});

describe('WorkStore', () => {
  it('loads work items and finds them by ticket/session', async () => {
    const store = new WorkStore();
    await store.load();
    expect(store.all).toHaveLength(8);
    const w = store.all[0]!;
    expect(store.forTicket(w.ticket!)?.id).toBe(w.id);
    expect(store.forSession(w.session_ids[0]!)?.id).toBe(w.id);
    store.apply({ type: 'work.updated', work: { ...w, state: { kind: 'finished' } } });
    expect(store.forTicket(w.ticket!)).toBeNull();
  });

  it('re-reads git status when a signal changes, not on other updates', async () => {
    const store = new WorkStore();
    await store.load();
    const w = store.all.find((x) => x.state.kind !== 'finished')!;
    const reads = (): number => mock.calls.filter((c) => c.cmd === 'work_status_all').length;
    store.apply({ type: 'work.updated', work: { ...w, pr_url: 'https://example.test/pr/1' } });
    expect(reads()).toBe(0);
    store.apply({ type: 'work.updated', work: { ...w, review_due: !w.review_due } });
    await flush();
    expect(reads()).toBe(1);
  });
});

describe('SettingsStore', () => {
  it('loads effective settings and reloads on settings.changed', async () => {
    const store = new SettingsStore();
    await store.load();
    expect(store.value()?.app.theme).toBe('system');
    const { settingsSet } = await import('$lib/ipc/commands');
    await settingsSet({ layer: 'global', path: 'app.theme', value: 'dark' });
    store.apply({
      type: 'settings.changed',
      layers: ['global'],
      paths: ['app.theme'],
      requires_restart: ['window.decorations'],
    });
    await flush();
    await flush();
    expect(store.value()?.app.theme).toBe('dark');
    expect(store.theme).toBe('dark');
    expect(store.sources()['app.theme']).toBe('global');
    expect(store.pendingRestart).toEqual(['window.decorations']);
  });

  it('tracks account statuses', () => {
    const store = new SettingsStore();
    store.apply({ type: 'account.status', account_id: 'jira-acme', status: 'needs_auth', detail: '401' });
    expect(store.accounts['jira-acme']).toEqual({ status: 'needs_auth', detail: '401' });
  });

  it('loads the schema once', async () => {
    const store = new SettingsStore();
    await store.loadSchema();
    await store.loadSchema();
    expect(mock.calls.filter((c) => c.cmd === 'settings_schema')).toHaveLength(1);
    expect(store.schema.data).toBeTruthy();
  });
});

describe('ToastsStore', () => {
  it('pushes, caps and auto-dismisses with one-shot timers', () => {
    vi.useFakeTimers();
    const store = new ToastsStore();
    store.apply({ type: 'toast', toast: { level: 'info', text: 'hi', action: null } });
    store.push({ level: 'error', text: 'sticky', action: null }, { timeoutMs: 0 });
    expect(store.list).toHaveLength(2);
    vi.advanceTimersByTime(5001);
    expect(store.list.map((t) => t.toast.text)).toEqual(['sticky']);
    for (let i = 0; i < 10; i += 1) store.info(`n${i}`);
    expect(store.list).toHaveLength(5);
    store.clear();
    expect(store.list).toHaveLength(0);
  });

  it('formats IPC errors', () => {
    const store = new ToastsStore();
    store.error(
      { code: 'needs_auth', message: '401 from Jira', detail: null, retry_after_ms: null },
      'Tickets',
    );
    expect(store.list[0]?.toast.text).toBe('Tickets: 401 from Jira');
  });
});

describe('UiStore', () => {
  it('handles overlays, sheets and ctl commands', () => {
    const store = new UiStore();
    const seen: string[] = [];
    store.onCtl((c) => seen.push(c.cmd));
    store.apply({ type: 'ctl.command', cmd: { cmd: 'palette' } });
    expect(store.overlay).toBe('palette');
    expect(seen).toEqual(['palette']);
    store.openSheet('start_work', { ticket: 'x' });
    store.openSheet('tool_picker');
    expect(store.sheet?.key).toBe('tool_picker');
    store.closeSheet('start_work');
    expect(store.sheets.map((s) => s.key)).toEqual(['tool_picker']);
    store.closeSheet();
    expect(store.sheet).toBeNull();
    store.toggleOverlay('palette');
    expect(store.overlay).toBeNull();
  });
});

describe('PluginsStore', () => {
  it('relays plugin.event to screen listeners', async () => {
    const store = new PluginsStore();
    await store.load();
    expect(store.plugins.data?.length).toBe(1);
    const got: string[] = [];
    const off = store.onScreenEvent('inst-1', (name) => got.push(name));
    store.apply({ type: 'plugin.event', instance_id: 'inst-1', name: 'session.bell', payload: null });
    store.apply({ type: 'plugin.event', instance_id: 'inst-2', name: 'other', payload: null });
    off();
    store.apply({ type: 'plugin.event', instance_id: 'inst-1', name: 'late', payload: null });
    expect(got).toEqual(['session.bell']);
  });
});

describe('UiEvent channel', () => {
  it('delivers mock events to listeners after subscribe', async () => {
    const got: UiEvent['type'][] = [];
    onAnyUiEvent((e) => got.push(e.type));
    await connectUiEvents();
    mock.emit({ type: 'project.removed', id: 'x' });
    await flush();
    expect(got).toEqual(['project.removed']);
  });
});
