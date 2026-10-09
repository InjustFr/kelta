// The mock must implement every command and its JSON fixtures must respect the generated types'
// enums and invariants (JSON imports are not type-checked literally).
import { describe, expect, it } from 'vitest';

import type { CiState, ErrorCode, ReviewKind, SessionStatus, StatusCategory, WorkKind } from '$lib/gen';
import { allPanes, validateLayout } from '$lib/layout';

import { COMMAND_NAMES, call, eventsSubscribe } from './commands';
import { MOCK_FIXTURES, createMockTransport, danglingSessions } from './mock';
import { setTransport } from './transport';

const SESSION_STATUS: SessionStatus[] = [
  'starting',
  'running',
  'working',
  'needs_input',
  'waiting_user',
  'done',
  'error',
  'exited',
  'unknown',
];
const ATTENTION = ['none', 'activity', 'done', 'error', 'needs_input'];
const LIFECYCLE = ['dormant', 'live', 'exited'];
const KINDS = ['shell', 'claude', 'editor', 'tool', 'setup', 'custom'];
const CATEGORIES: StatusCategory[] = ['todo', 'in_progress', 'in_review', 'done', 'unknown'];
const CI: CiState[] = ['success', 'failure', 'pending', 'error', 'none'];
const REVIEW_KINDS: ReviewKind[] = ['review_requested', 'authored'];
const WORK_KINDS: WorkKind[] = ['ticket', 'review', 'branch'];
const PANE_KINDS = [
  'terminal',
  'web',
  'plugin_screen',
  'tickets',
  'ticket_detail',
  'reviews',
  'review_detail',
  'inbox',
  'work_item',
  'settings',
  'diagnostics',
  'welcome',
  'empty',
];

describe('mock fixtures', () => {
  it('has 3 projects + Home, 10 sessions, tickets for every provider, reviews and work items', () => {
    expect(MOCK_FIXTURES.projects.filter((p) => !p.builtin)).toHaveLength(3);
    expect(MOCK_FIXTURES.projects.filter((p) => p.builtin)).toHaveLength(1);
    expect(MOCK_FIXTURES.sessions).toHaveLength(10);
    const accounts = new Set(MOCK_FIXTURES.tickets.map((t) => t.ticket.ref.account));
    expect([...accounts].sort()).toEqual(['github-oss', 'gitlab-corp', 'jira-acme', 'redmine-corp']);
    expect(MOCK_FIXTURES.reviews.length).toBeGreaterThanOrEqual(5);
    expect(MOCK_FIXTURES.reviews.some((r) => r.project_ids.length === 0)).toBe(true);
    expect(MOCK_FIXTURES.work.length).toBeGreaterThanOrEqual(3);
  });

  it('projects are well-formed', () => {
    const ids = new Set<string>();
    for (const p of MOCK_FIXTURES.projects) {
      expect(p.id).toMatch(/^[a-z0-9-]{1,40}$/);
      expect(ids.has(p.id)).toBe(false);
      ids.add(p.id);
      expect(ATTENTION).toContain(p.attention.level);
      for (const r of p.repos) expect(typeof r.path).toBe('string');
      if (p.tracker) {
        expect(p.tracker.views.length).toBeGreaterThan(0);
        for (const v of p.tracker.views) expect(Object.keys(v)).toHaveLength(16);
      }
    }
    expect(MOCK_FIXTURES.projects.filter((p) => p.active)).toHaveLength(1);
  });

  it('sessions use valid enums and reference known projects', () => {
    const projects = new Set(MOCK_FIXTURES.projects.map((p) => p.id));
    for (const s of MOCK_FIXTURES.sessions) {
      expect(SESSION_STATUS).toContain(s.status);
      expect(ATTENTION).toContain(s.attention);
      expect(LIFECYCLE).toContain(s.lifecycle);
      expect(KINDS).toContain(s.kind.type);
      expect(projects.has(s.project_id)).toBe(true);
      expect(Object.keys(s)).toHaveLength(19);
    }
  });

  it('layouts are valid and only show existing sessions', () => {
    for (const [id, layout] of Object.entries(MOCK_FIXTURES.layouts)) {
      expect(layout.project_id).toBe(id);
      expect(validateLayout(layout)).toEqual([]);
      for (const tab of layout.tabs)
        for (const p of allPanes(tab.root)) expect(PANE_KINDS).toContain(p.content.kind);
    }
    expect(danglingSessions(MOCK_FIXTURES)).toEqual([]);
  });

  it('tickets, reviews and work items use valid enums', () => {
    for (const t of MOCK_FIXTURES.tickets) expect(CATEGORIES).toContain(t.ticket.status.category);
    for (const r of MOCK_FIXTURES.reviews) {
      expect(CI).toContain(r.review.ci);
      expect(REVIEW_KINDS).toContain(r.review.kind);
      expect(r.review.head_sha).toHaveLength(40);
    }
    for (const w of MOCK_FIXTURES.work) {
      expect(WORK_KINDS).toContain(w.kind);
      expect(w.steps.length).toBe(12);
    }
  });
});

describe('mock transport', () => {
  it('implements every command (no Unsupported for well-formed calls)', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    const s = controls.state;
    const ticket = s.tickets[0]!.ticket.ref;
    const review = s.reviews[0]!.review;
    const session = s.sessions[0]!;
    const work = s.work[0]!;
    const ctx = {
      repo_id: null,
      cwd: null,
      session_id: null,
      work_item_id: null,
      ticket: null,
      review: null,
      extra: {},
    };
    const args: Record<string, Record<string, unknown>> = {
      app_ready: { t_ms: 1 },
      open_external: { url: 'https://example.com' },
      clipboard_read: { kind: 'clipboard' },
      clipboard_write: { kind: 'primary', text: 'x' },
      settings_layer_get: { layer: 'global' },
      settings_set: { layer: 'global', path: 'app.theme', value: 'dark' },
      settings_reset: { layer: 'global', path: 'app.theme' },
      settings_validate: { layer: 'global', text: 'a = 1' },
      settings_write_raw: { layer: 'global', text: 'a = 1' },
      settings_open_file: { layer: 'global' },
      repo_trust: { project_id: 'shop', repo_id: 'api', trust: true },
      secret_set: { secret_ref: 'keyring:x', value: 'v' },
      secret_delete: { secret_ref: 'keyring:x' },
      account_test: { account_id: 'jira-acme' },
      project_detect: { path: '/Users/ada/code/new-thing' },
      project_update: {
        id: 'shop',
        patch: {
          name: 'Shop!',
          color: null,
          icon: null,
          default_template: null,
          repos: null,
          tracker: null,
          remove_tracker: false,
        },
      },
      project_open: { id: 'shop' },
      project_close: { id: 'kelta-tools', kill_sessions: false },
      project_activate: { id: 'billing' },
      project_reorder: { ids: ['billing', 'shop'] },
      layout_get: { project_id: 'shop' },
      session_spawn: {
        req: {
          project_id: 'home',
          kind: { type: 'shell' },
          name: null,
          program: null,
          args: [],
          cwd: null,
          env: {},
          cols: 80,
          rows: 24,
          work_item_id: null,
          restore: { kind: 'none' },
          close_on_exit: 'never',
          template_id: null,
        },
      },
      session_spawn_template: { project_id: 'home', template_id: 'claude+editor', ctx, placement: 'new_tab' },
      session_detach: { id: session.id, generation: 1 },
      session_resize: { id: session.id, cols: 100, rows: 30 },
      session_ack: { id: session.id, generation: 1, bytes: 10 },
      session_restart: { id: session.id },
      session_rename: { id: session.id, name: 'n' },
      session_mark_seen: { id: session.id },
      session_link: { id: session.id, work_item_id: work.id },
      session_text_tail: { id: session.id, max_lines: 3 },
      terminal_set_palette: { palette: { foreground: '#fff', background: '#000', cursor: '#fff', ansi: [] } },
      tracker_list: { scope: { kind: 'all' }, refresh: false },
      tracker_get: { ticket },
      tracker_columns: { project_id: 'shop' },
      tracker_transitions: { ticket },
      tracker_transition: { ticket, transition_id: 'to-in_review' },
      tracker_move: { ticket, column_id: 'done' },
      tracker_comment: { ticket, markdown: 'hi' },
      tracker_assign: { ticket, assignee: { kind: 'me' } },
      tracker_search: { scope: { kind: 'all' }, text: 'rate' },
      review_list: { scope: { kind: 'all' }, kind: 'authored', refresh: false },
      review_get: { review: review.ref },
      review_approve: { review: review.ref, head_sha: review.head_sha },
      review_comment: { review: review.ref, body: 'lgtm' },
      review_request_changes: { review: review.ref, body: 'nope' },
      work_plan: { project_id: 'shop', source: { kind: 'ticket', ticket: s.tickets[1]!.ticket.ref } },
      work_list: {},
      work_resume: { id: work.id },
      work_retry_step: { id: work.id, step: 'persist' },
      work_create_pr: { id: work.id, draft: { title: null, body: null, draft: null } },
      work_status: { id: work.id },
      editor_open: { target: { kind: 'session', id: session.id }, path: '/x', line: 3 },
      editor_send_selection: { editor_session: s.sessions[1]!.id, claude_session: session.id },
      tool_list: { project_id: 'shop' },
      tool_check: { tool_id: 'sl-web' },
      tool_open: { project_id: 'shop', tool_id: 'lazygit', ctx, placement: 'split_right' },
      tool_close: { instance_id: 'x' },
      plugin_inspect: { source: '/tmp/plugin' },
      plugin_install: { source: '/tmp/plugin', sha256: 'x', grant: [] },
      plugin_enable: { id: s.plugins[0]!.id, enabled: false },
      plugin_grant: { id: s.plugins[0]!.id, permissions: ['projects.read'] },
      plugin_screen_open: { plugin_id: s.plugins[0]!.id, screen_id: 'main', params: null },
      command_run: { command_id: 'x', ctx },
      trigger_list: {},
      trigger_test: { trigger_id: 't', payload: {} },
      trigger_log: { limit: 5 },
      // last: destructive ones
      work_finish: {
        id: work.id,
        opts: { remove_worktree: true, delete_branch: false, force: true, transition_to: null },
      },
      session_kill: { id: s.sessions[2]!.id, force: false },
      project_remove: { id: 'kelta-tools', kill_sessions: true },
      plugin_uninstall: { id: 'nope' },
    };
    const special = new Set([
      'events_subscribe',
      'session_attach',
      'session_write',
      'work_start',
      'work_link',
      'layout_save',
      'project_create',
      'plugin_call',
      'plugin_screen_close',
    ]);
    const order = [...COMMAND_NAMES.filter((c) => !(c in args) && !special.has(c)), ...Object.keys(args)];
    for (const cmd of order) {
      await call(cmd as never, (args[cmd] ?? {}) as never).catch(
        (e: { code: ErrorCode; message: string }) => {
          throw new Error(`${cmd}: ${e.code} ${e.message}`);
        },
      );
    }
    await eventsSubscribe(() => {});
    const plan = await call('work_plan', {
      project_id: 'shop',
      source: { kind: 'branch', name: 'spike', task: null, repo: null },
    });
    const started = await call('work_start', { plan });
    expect(started.session_ids.length).toBeGreaterThan(0);
    // Scratch item: wip/ branch from the task, title, no adoption, then linked to a ticket.
    const task = { kind: 'branch' as const, name: '', task: 'Speed up search\nKeep ranking', repo: null };
    const scratch = await call('work_plan', { project_id: 'shop', source: task });
    expect(scratch.branch).toBe('wip/speed-up-search');
    expect(scratch.claude.prompt).toBe('Speed up search\nKeep ranking');
    const wip = await call('work_start', { plan: scratch });
    expect(wip.title).toBe('Speed up search');
    await expect(call('work_plan', { project_id: 'shop', source: task })).rejects.toMatchObject({
      code: 'conflict',
    });
    const linked = await call('work_link', {
      id: wip.id,
      ticket: { account: 'jira-acme', key: 'SHOP-142', id: '10142' },
      apply_side_effects: true,
    });
    expect(linked).toMatchObject({ kind: 'ticket', branch: 'wip/speed-up-search' });
    const layout = await call('layout_get', { project_id: 'shop' });
    await expect(call('layout_save', { layout })).resolves.toEqual({ rev: layout.rev + 1 });
    await expect(call('layout_save', { layout })).rejects.toMatchObject({ code: 'conflict' });
    const draft = await call('project_detect', { path: '/x/new-proj' });
    await expect(call('project_create', { draft })).resolves.toMatchObject({ id: 'new-proj' });
    const screen = await call('plugin_screen_open', {
      plugin_id: s.plugins[0]!.id,
      screen_id: 'main',
      params: null,
    });
    await expect(
      call('plugin_call', { instance_id: screen.instance_id, method: 'app.info', params: null }),
    ).resolves.toBeTruthy();
    await expect(call('plugin_screen_close', { instance_id: screen.instance_id })).resolves.toBeNull();
  });

  it('plugin_call without permission is PermissionDenied', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    const plugin = controls.state.plugins[0]!;
    plugin.granted = [];
    const screen = await call('plugin_screen_open', {
      plugin_id: plugin.id,
      screen_id: 'main',
      params: null,
    });
    await expect(
      call('plugin_call', { instance_id: screen.instance_id, method: 'projects.list', params: null }),
    ).rejects.toMatchObject({
      code: 'permission_denied',
    });
  });

  it('tracker_transition requiring fields returns NeedsFields with detail.fields', async () => {
    const { transport } = createMockTransport();
    setTransport(transport);
    const ticket = { account: 'jira-acme', key: 'SHOP-151', id: '10151' };
    await expect(call('tracker_transition', { ticket, transition_id: 'to-done' })).rejects.toMatchObject({
      code: 'needs_fields',
      detail: { fields: [{ id: 'resolution', name: 'Resolution', required: true }] },
    });
  });

  it('work_finish on a dirty worktree returns Dirty unless forced', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    const dirty = controls.state.work[1]!;
    const opts = { remove_worktree: true, delete_branch: false, force: false, transition_to: null };
    await expect(call('work_finish', { id: dirty.id, opts })).rejects.toMatchObject({ code: 'dirty' });
    await expect(
      call('work_finish', { id: dirty.id, opts: { ...opts, force: true } }),
    ).resolves.toMatchObject({
      state: { kind: 'finished' },
    });
  });

  it('reset restores the fixtures', async () => {
    const { transport, controls } = createMockTransport();
    setTransport(transport);
    await call('project_remove', { id: 'shop', kill_sessions: true });
    expect(controls.state.projects.some((p) => p.id === 'shop')).toBe(false);
    controls.reset();
    expect(controls.state.projects.some((p) => p.id === 'shop')).toBe(true);
    expect(controls.calls).toHaveLength(0);
  });
});
