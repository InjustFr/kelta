import { describe, expect, it } from 'vitest';

import type { ClaudeRun, RunClaim, RunState, SessionInfo } from '$lib/gen';

import { paneTitle, runLine, sessionTitle, toolIcon } from './labels';

const tool = (id: string, icon: string | null = null, label = id) => ({ id, label, icon });

describe('toolIcon (#377)', () => {
  it('honours a Lucide name or 1-2 characters, then a well-known tool, then the monogram', () => {
    expect(toolIcon(tool('k9s', 'cpu'))).toBe('cpu');
    expect(toolIcon(tool('k9s', 'ship-wheel'))).toBe('ship-wheel'); // outside the startup sprite (#388)
    expect(toolIcon(tool('k9s', 'K'))).toBe('K');
    expect(toolIcon(tool('k9s', 'K9'))).toBe('K9');
    expect(toolIcon(tool('lazygit', 'branch'))).toBe('git-branch'); // unknown name: not text either
    expect(toolIcon(tool('lazydocker'))).toBe('container');
    expect(toolIcon(tool('my-shell', null, 'Zsh'))).toBe('terminal');
    expect(toolIcon(tool('k9s'))).toBe('k9');
  });
});

describe('runLine (#392)', () => {
  const run = (id: string, session_id = 's1', name = 'pass', status = 'running'): ClaudeRun => ({
    run_id: id,
    session_id,
    name,
    status,
    phases: [
      { title: 'Pick', agents: [{ id: 'a', label: 'pick', done: true }] },
      { title: 'Implement', agents: [{ id: 'b', label: 'impl', done: false }] },
    ],
    duration_ms: null,
    total_tokens: null,
    total_tool_calls: null,
    result: null,
    logs: [],
  });
  const claim = (state: RunState, phase: string | null = 'Review'): RunClaim => ({
    name: 'pass',
    run_id: 'r',
    session_id: 's1',
    phase,
    state,
    note: null,
    sha: null,
  });

  it("a work item's claim shows its phase, or its state once it is not running", () => {
    expect(runLine(claim('running'), [], [])).toEqual({ text: 'pass · Review', sessionId: 's1' });
    expect(runLine(claim('failed'), [], [])?.text).toBe('pass · failed');
    expect(runLine(claim('done'), [], [])).toBeNull();
  });

  it('running runs no live claim names, counted past the first', () => {
    expect(runLine(null, [run('a')], [])).toEqual({ text: 'pass · Implement', sessionId: 's1' });
    expect(
      runLine(null, [run('a'), run('b', 's1', 'triage'), run('c', 's1', 'pass', 'completed')], [])?.text,
    ).toBe('pass · Implement +1');
    // A live claim takes one run of its session and name; a finished claim takes none.
    expect(runLine(null, [run('a'), run('b')], [claim('running')])?.text).toBe('pass · Implement');
    expect(runLine(null, [run('a')], [claim('running')])).toBeNull();
    expect(runLine(null, [run('a')], [claim('done')])?.text).toBe('pass · Implement');
    expect(runLine(null, [run('a', 's2')], [claim('running')])?.text).toBe('pass · Implement');
  });
});

describe('sessionTitle (#450)', () => {
  it('is the terminal title without its status glyph, else the name', () => {
    expect(sessionTitle({ name: 'claude', title: '✳ Kelta UI and IDE UX' })).toBe('Kelta UI and IDE UX');
    expect(sessionTitle({ name: 'claude', title: '⠂ Kelta UI and IDE UX' })).toBe('Kelta UI and IDE UX');
    expect(sessionTitle({ name: 'claude', title: '⠐ Kelta UI and IDE UX' })).toBe('Kelta UI and IDE UX');
    expect(sessionTitle({ name: 'claude', title: null })).toBe('claude');
    expect(sessionTitle({ name: 'claude', title: '  ✳ ' })).toBe('claude');
  });

  it('names the pane too', () => {
    const session = { name: 'claude', title: '✳ Kelta UI' } as SessionInfo;
    expect(paneTitle({ kind: 'terminal', session_id: 's' }, session)).toBe('Kelta UI');
  });
});
