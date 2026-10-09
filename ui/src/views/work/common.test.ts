import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';

import {
  accountErrorBanner,
  columnFor,
  parseCandidates,
  parseDirtyFiles,
  parseFields,
  planValid,
  validateBranch,
  validatePlan,
} from './common';

describe('validateBranch', () => {
  it('accepts normal names', () => {
    expect(validateBranch('feat/SHOP-142-rate-limit-login')).toBeNull();
    expect(validateBranch('kelta/pr-12')).toBeNull();
  });

  it.each([
    '',
    ' lead',
    '-dash',
    'a b',
    'a..b',
    'a~b',
    'a^b',
    'a:b',
    'a?b',
    'a*b',
    'a[b',
    'a\\b',
    'a@{b',
    '/abs',
    'trail/',
    'dou//ble',
    'dot.',
    'x/.hidden',
    'x/y.lock',
    '@',
  ])('rejects %j', (name) => {
    expect(validateBranch(name)).not.toBeNull();
  });
});

describe('validatePlan', () => {
  const base = () => structuredClone(samples.startWorkPlan);

  it('accepts the sample plan', () => {
    expect(planValid(validatePlan(base()))).toBe(true);
  });

  it('flags an empty prompt, a bad branch and an empty comment', () => {
    const plan = base();
    plan.claude.prompt = '   ';
    plan.branch = 'bad..name';
    plan.side_effects.comment = ' ';
    const errors = validatePlan(plan);
    expect(errors.prompt).not.toBeNull();
    expect(errors.branch).not.toBeNull();
    expect(errors.comment).not.toBeNull();
    expect(planValid(errors)).toBe(false);
  });

  it('requires a repo that is one of the choices', () => {
    const plan = base();
    plan.repo_choices = ['api', 'web'];
    plan.repo_id = 'nope';
    expect(validatePlan(plan).repo).not.toBeNull();
  });
});

describe('error detail parsing', () => {
  it('parses fields given as strings or objects', () => {
    expect(
      parseFields({ fields: ['resolution', { id: 'fix', name: 'Fix version', required: false }] }),
    ).toEqual([
      { id: 'resolution', name: 'resolution', required: true, options: [] },
      { id: 'fix', name: 'Fix version', required: false, options: [] },
    ]);
    expect(
      parseFields({ fields: [{ id: 'r', allowed_values: [{ id: '1', name: 'Fixed' }, 'Wontfix'] }] })[0]
        ?.options,
    ).toEqual([
      { value: '1', label: 'Fixed' },
      { value: 'Wontfix', label: 'Wontfix' },
    ]);
    expect(parseFields(null)).toEqual([]);
  });

  it('parses transition candidates', () => {
    const c = parseCandidates({
      candidates: [
        { id: 'a', name: 'Start', to: { id: 's', name: 'Doing', category: 'in_progress' } },
        { id: 'b' },
      ],
    });
    expect(c.map((t) => [t.id, t.name, t.to.name])).toEqual([
      ['a', 'Start', 'Doing'],
      ['b', 'b', 'b'],
    ]);
  });

  it('parses dirty files', () => {
    expect(parseDirtyFiles({ files: ['a.rs', 3, 'b.rs'] })).toEqual(['a.rs', 'b.rs']);
    expect(parseDirtyFiles(null)).toEqual([]);
  });
});

describe('banners and columns', () => {
  const err = (code: 'needs_auth' | 'rate_limited' | 'network', retry: number | null = null) => ({
    account_id: 'jira-acme',
    error: { code, message: 'boom', detail: null, retry_after_ms: retry },
  });

  it('describes account failures', () => {
    expect(accountErrorBanner(err('needs_auth')).reauth).toBe(true);
    expect(accountErrorBanner(err('rate_limited', 60_000), 0).text).toMatch(/retrying at \d\d:\d\d/);
    expect(accountErrorBanner(err('network')).text).toMatch(/Offline/);
  });

  it('places statuses in columns by name then category', () => {
    const cols = [
      { id: 'todo', name: 'To do', category: 'todo', match_names: [] },
      { id: 'doing', name: 'Doing', category: 'in_progress', match_names: ['In Progress'] },
      { id: 'rev', name: 'Review', category: 'in_review', match_names: [] },
    ];
    expect(columnFor(cols, { name: 'in progress', category: 'in_progress' })?.id).toBe('doing');
    expect(columnFor(cols, { name: 'Weird', category: 'in_review' })?.id).toBe('rev');
    expect(columnFor(cols, { name: 'Weird', category: 'done' })).toBeNull();
  });
});
