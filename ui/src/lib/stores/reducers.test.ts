import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';
import type { ProjectInfo, SessionInfo, UiEvent } from '$lib/gen';

import {
  attentionFromProjects,
  maxAttention,
  nextNeedingInput,
  reduceAccounts,
  reduceAttention,
  reduceLayouts,
  reduceProjects,
  reduceSessions,
  reduceWork,
  reviewKey,
  scopeAffects,
  scopeKey,
  sessionsToMap,
} from './reducers';

const project = (id: string, extra: Partial<ProjectInfo> = {}): ProjectInfo => ({
  ...samples.projectInfo,
  id,
  name: id,
  active: false,
  ...extra,
});
const session = (id: string, extra: Partial<SessionInfo> = {}): SessionInfo => ({
  ...samples.sessionInfo,
  id,
  ...extra,
});

describe('helpers', () => {
  it('maxAttention follows the enum order', () => {
    expect(maxAttention([])).toBe('none');
    expect(maxAttention(['activity', 'done'])).toBe('done');
    expect(maxAttention(['needs_input', 'error', 'activity'])).toBe('needs_input');
  });

  it('scope keys and overlap', () => {
    expect(scopeKey({ kind: 'all' })).toBe('all');
    expect(scopeKey({ kind: 'project', id: 'shop' })).toBe('project:shop');
    expect(scopeAffects({ kind: 'project', id: 'a' }, { kind: 'all' })).toBe(true);
    expect(scopeAffects({ kind: 'all' }, { kind: 'project', id: 'a' })).toBe(true);
    expect(scopeAffects({ kind: 'project', id: 'a' }, { kind: 'project', id: 'b' })).toBe(false);
  });

  it('review keys', () => {
    expect(reviewKey({ account: 'gh', repo: 'acme/shop', number: 3 })).toBe('gh:acme/shop#3');
  });
});

describe('reduceProjects', () => {
  it('upserts and keeps a single active project', () => {
    const list = [project('a', { active: true }), project('b')];
    const next = reduceProjects(list, { type: 'project.updated', project: project('b', { active: true }) });
    expect(next.map((p) => [p.id, p.active])).toEqual([
      ['a', false],
      ['b', true],
    ]);
    const added = reduceProjects(next, { type: 'project.updated', project: project('c') });
    expect(added.map((p) => p.id)).toEqual(['a', 'b', 'c']);
  });

  it('removes projects', () => {
    expect(reduceProjects([project('a')], { type: 'project.removed', id: 'a' })).toEqual([]);
  });

  it('applies attention.changed', () => {
    const next = reduceProjects([project('a')], {
      type: 'attention.changed',
      project_id: 'a',
      level: 'error',
      needs_input_count: 2,
      total_needs_input: 2,
    });
    expect(next[0]?.attention).toEqual({ level: 'error', needs_input: 2 });
  });

  it('returns the same list for unrelated events', () => {
    const list = [project('a')];
    expect(reduceProjects(list, { type: 'session.removed', id: 'x' })).toBe(list);
  });
});

describe('reduceSessions', () => {
  it('upserts, removes and drops sessions of removed projects', () => {
    let map = sessionsToMap([session('1', { project_id: 'a' }), session('2', { project_id: 'b' })]);
    map = reduceSessions(map, { type: 'session.updated', session: session('3', { project_id: 'a' }) });
    expect(Object.keys(map).sort()).toEqual(['1', '2', '3']);
    map = reduceSessions(map, { type: 'session.removed', id: '1' });
    expect(Object.keys(map).sort()).toEqual(['2', '3']);
    map = reduceSessions(map, { type: 'project.removed', id: 'a' });
    expect(Object.keys(map)).toEqual(['2']);
  });

  it('ignores removing an unknown session', () => {
    const map = sessionsToMap([session('1')]);
    expect(reduceSessions(map, { type: 'session.removed', id: 'zz' })).toBe(map);
  });
});

describe('nextNeedingInput', () => {
  const list = [
    session('1', { project_id: 'b', attention: 'needs_input', created_at: '2026-01-01T00:00:00Z' }),
    session('2', { project_id: 'a', attention: 'none', status: 'running' }),
    session('3', { project_id: 'a', attention: 'needs_input', created_at: '2026-01-02T00:00:00Z' }),
  ];

  it('walks needing-input sessions in project order and wraps', () => {
    expect(nextNeedingInput(list, ['a', 'b'], null)?.id).toBe('3');
    expect(nextNeedingInput(list, ['a', 'b'], '3')?.id).toBe('1');
    expect(nextNeedingInput(list, ['a', 'b'], '1')?.id).toBe('3');
  });

  it('returns null when nothing needs input', () => {
    expect(nextNeedingInput([session('x', { attention: 'none', status: 'running' })], [], null)).toBeNull();
  });
});

describe('reduceLayouts', () => {
  it('replaces on layout.changed', () => {
    const ev = samples.uiEventLayoutChanged as Extract<UiEvent, { type: 'layout.changed' }>;
    const map = reduceLayouts({}, ev);
    expect(map[ev.project_id]).toEqual(ev.layout);
  });

  it('applies ui.open to a loaded layout only', () => {
    const ev = samples.uiEventUiOpen as Extract<UiEvent, { type: 'ui.open' }>;
    expect(reduceLayouts({}, ev)).toEqual({});
    const loaded = { [ev.project_id]: { ...samples.layout, project_id: ev.project_id } };
    const next = reduceLayouts(loaded, ev);
    expect(next[ev.project_id]).not.toBe(loaded[ev.project_id]);
    const tabs = next[ev.project_id]?.tabs ?? [];
    expect(JSON.stringify(tabs)).toContain(JSON.stringify(ev.request.content.kind));
  });
});

describe('reduceWork', () => {
  it('upserts work items', () => {
    const map = reduceWork({}, { type: 'work.updated', work: samples.workItem });
    expect(map[samples.workItem.id]).toEqual(samples.workItem);
    const failed = reduceWork(map, {
      type: 'work.updated',
      work: { ...samples.workItem, state: samples.workItemFailed.state },
    });
    expect(failed[samples.workItem.id]?.state.kind).toBe('failed');
    expect(reduceWork(failed, { type: 'project.removed', id: samples.workItem.project_id })).toEqual({});
  });
});

describe('reduceAttention', () => {
  it('seeds from projects and applies events', () => {
    let s = attentionFromProjects([
      project('a', { attention: { level: 'needs_input', needs_input: 2 } }),
      project('b', { attention: { level: 'none', needs_input: 0 } }),
    ]);
    expect(s.totalNeedsInput).toBe(2);
    s = reduceAttention(s, {
      type: 'attention.changed',
      project_id: 'b',
      level: 'needs_input',
      needs_input_count: 1,
      total_needs_input: 3,
    });
    expect(s.byProject.b).toEqual({ level: 'needs_input', needs_input_count: 1 });
    expect(s.totalNeedsInput).toBe(3);
    s = reduceAttention(s, { type: 'project.removed', id: 'a' });
    expect(s.totalNeedsInput).toBe(1);
    expect(s.byProject.a).toBeUndefined();
  });
});

describe('reduceAccounts', () => {
  it('tracks account.status', () => {
    const ev = samples.uiEventAccountStatus as Extract<UiEvent, { type: 'account.status' }>;
    const map = reduceAccounts({}, ev);
    expect(map[ev.account_id]).toEqual({ status: ev.status, detail: ev.detail });
  });
});
