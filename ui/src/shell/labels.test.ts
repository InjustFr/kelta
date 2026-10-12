import { describe, expect, it } from 'vitest';

import type { ClaudeRun, SessionInfo } from '$lib/gen';

import { runState, toolIcon } from './labels';

const tool = (id: string, icon: string | null = null, label = id) => ({ id, label, icon });

describe('toolIcon (#377)', () => {
  it('honours a sprite name or 1-2 characters, then a well-known tool, then the monogram', () => {
    expect(toolIcon(tool('k9s', 'cpu'))).toBe('cpu');
    expect(toolIcon(tool('k9s', 'K'))).toBe('K');
    expect(toolIcon(tool('k9s', 'K9'))).toBe('K9');
    expect(toolIcon(tool('lazygit', 'branch'))).toBe('git-branch'); // unknown name: not text either
    expect(toolIcon(tool('lazydocker'))).toBe('box');
    expect(toolIcon(tool('my-shell', null, 'Zsh'))).toBe('terminal');
    expect(toolIcon(tool('k9s'))).toBe('k9');
  });
});

describe('runState (#377)', () => {
  const run = (status: string): ClaudeRun => ({
    run_id: 'wf_1',
    session_id: 's1',
    name: 'pass',
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
  const session = (status: string) => ({ status, attention: 'none' }) as unknown as SessionInfo;

  it('running: the live phase; asking: needs input; ended: its status', () => {
    expect(runState(run('running'), session('working'))).toEqual({ label: 'Implement', lamp: 'working' });
    expect(runState(run('running'), session('needs_input'))).toEqual({
      label: 'Needs input',
      lamp: 'needs_input',
    });
    expect(runState(run('completed'), session('done'))).toEqual({ label: 'completed', lamp: 'none' });
    expect(runState(run('failed'), session('done')).lamp).toBe('error');
  });
});
