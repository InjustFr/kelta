import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';

import type { ClaudeRun } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { applyUiEvent, sessions } from '$lib/stores';

import WorkflowRunsPane from './WorkflowRunsPane.svelte';

let mock: MockControls;
let sid: string;

const run = (patch: Partial<ClaudeRun> = {}): ClaudeRun => ({
  run_id: 'wf_1',
  session_id: sid,
  name: 'kelta-ticket-pass',
  status: 'running',
  phases: [
    { title: 'Implement', agents: [{ id: 'a1', label: 'implement-#7', done: true }] },
    { title: 'Review', agents: [{ id: 'a2', label: 'review-#7-r1', done: false }] },
  ],
  duration_ms: null,
  total_tokens: null,
  total_tool_calls: null,
  result: null,
  logs: [],
  ...patch,
});

const paneProps = () => ({
  projectId: 'shop',
  tabId: 't',
  paneId: 'p',
  content: { kind: 'claude_runs' as const, session_id: sid },
  visible: true,
  focused: true,
});

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  await sessions.load();
  sid = mock.state.sessions[0]!.id;
  sessions.runs = {};
});

describe('WorkflowRunsPane', () => {
  it('shows phases and agents live, the selected agent tail, then the final totals', async () => {
    applyUiEvent({ type: 'workflow.updated', run: run() });
    render(WorkflowRunsPane, { props: paneProps() });
    const agents = screen.getAllByTestId('workflow-agent');
    expect(agents.map((a) => [a.textContent, a.dataset.state])).toEqual([
      ['✓implement-#7', 'done'],
      ['●review-#7-r1', 'running'],
    ]);

    await fireEvent.click(agents[1]!);
    await waitFor(() =>
      expect(screen.getByTestId('workflow-tail').textContent).toContain('a2: reading the code'),
    );
    const tails = () => mock.calls.filter((c) => c.cmd === 'claude_run_tail').length;
    expect(mock.calls.find((c) => c.cmd === 'claude_run_tail')?.args).toEqual({
      id: sid,
      run_id: 'wf_1',
      agent_id: 'a2',
    });

    // An update of the run reads the tail again; the final record brings the totals.
    applyUiEvent({
      type: 'workflow.updated',
      run: run({
        status: 'killed',
        duration_ms: 61_000,
        total_tokens: 1234,
        total_tool_calls: 7,
        result: '{"x":1}',
        logs: ['Picked #7'],
      }),
    });
    await waitFor(() => expect(tails()).toBe(2));
    const section = screen.getByTestId('workflow-run');
    expect(section.dataset.status).toBe('killed');
    expect(section.textContent).toContain('1m 1s · 1,234 tokens · 7 tool calls');
    expect(section.textContent).toContain('Picked #7');
    expect(screen.getAllByTestId('workflow-agent')[1]!.dataset.state).toBe('stopped');
  });

  it('a run with nothing known yet still renders', () => {
    applyUiEvent({ type: 'workflow.updated', run: run({ name: null, phases: [] }) });
    render(WorkflowRunsPane, { props: paneProps() });
    expect(screen.getByTestId('workflow-run').textContent).toContain('wf_1');
  });

  it('loads the runs core saw once per session, and drops them with the session', async () => {
    // Not `sid`: the pane rendered above already loaded its runs.
    const other = mock.state.sessions[1]!.id;
    mock.state.claudeRuns.push(run({ session_id: other }));
    await sessions.loadRuns(other);
    await sessions.loadRuns(other);
    expect(mock.calls.filter((c) => c.cmd === 'claude_runs_list')).toHaveLength(1);
    expect(sessions.runs[other]).toHaveLength(1);
    applyUiEvent({ type: 'session.removed', id: other });
    expect(sessions.runs[other]).toBeUndefined();
  });
});
