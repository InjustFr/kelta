import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { StartWorkPlan, WorkItem } from '$lib/gen';
import { workPlan } from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { applyUiEvent, projects, toasts, work } from '$lib/stores';

import StartWorkSheet from './StartWorkSheet.svelte';

let mock: MockControls;

const ticket = { account: 'jira-acme', key: 'SHOP-151', id: '10151' };

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  work.byId = {};
  await projects.load();
});

async function mountSheet(
  mutate: (p: StartWorkPlan) => void = () => {},
  latency = false,
): Promise<{ onclose: ReturnType<typeof vi.fn>; plan: StartWorkPlan }> {
  if (latency) {
    const created = createMockTransport({ latencyMs: 120 });
    mock = created.controls;
    setTransport(created.transport);
  }
  const plan = await workPlan({ project_id: 'shop', source: { kind: 'ticket', ticket } });
  mutate(plan);
  const onclose = vi.fn();
  render(StartWorkSheet, { props: { plan, onclose } });
  return { onclose, plan };
}

const start = () => screen.getByRole('button', { name: 'Start' });
const sentPlan = (): StartWorkPlan =>
  (mock.calls.filter((c) => c.cmd === 'work_start').at(-1)?.args as { plan: StartWorkPlan }).plan;

describe('StartWorkSheet plan', () => {
  it('shows the editable plan', async () => {
    await mountSheet();
    expect(screen.getByRole('dialog', { name: /Start work on SHOP-151/ })).toBeTruthy();
    expect((screen.getByLabelText('Branch') as HTMLInputElement).value).toMatch(/^feat\/shop-151/);
    expect((screen.getByLabelText('Prompt') as HTMLTextAreaElement).value.length).toBeGreaterThan(0);
    expect(screen.getByRole('switch', { name: /Assign the ticket to me/ })).toBeTruthy();
    expect(screen.getByRole('switch', { name: /Run setup commands/ })).toBeTruthy();
  });

  it('refuses an empty prompt and a malformed branch', async () => {
    await mountSheet();
    const branch = screen.getByLabelText('Branch');
    await fireEvent.input(branch, { target: { value: 'bad..name' } });
    expect(await screen.findByText(/must not contain "\.\."/)).toBeTruthy();

    await fireEvent.input(branch, { target: { value: 'feat/ok' } });
    await fireEvent.input(screen.getByLabelText('Prompt'), { target: { value: '   ' } });
    await fireEvent.click(start());
    expect(await screen.findByText('The prompt must not be empty')).toBeTruthy();
    expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(false);
  });

  it('sends the edited plan, the side-effect toggles and closes', async () => {
    const { onclose } = await mountSheet();
    await fireEvent.input(screen.getByLabelText('Branch'), { target: { value: 'feat/custom-branch' } });
    await fireEvent.input(screen.getByLabelText('Prompt'), { target: { value: 'Do the thing' } });
    await fireEvent.click(screen.getByRole('switch', { name: /Run setup commands/ }));
    await fireEvent.click(start());
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    const plan = sentPlan();
    expect(plan.branch).toBe('feat/custom-branch');
    expect(plan.claude.prompt).toBe('Do the thing');
    expect(plan.side_effects.run_setup).toBe(false);
    expect(work.all.some((w) => w.branch === 'feat/custom-branch')).toBe(true);
  });

  it('submits with Ctrl+Enter', async () => {
    const { onclose } = await mountSheet();
    await fireEvent.keyDown(screen.getByTestId('start-work-form'), { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(true);
  });

  it('offers the collision choice when the branch exists', async () => {
    const { onclose } = await mountSheet((p) => {
      p.branch_exists = { has_worktree: true, choice: 'reuse' };
    });
    expect(screen.getByText(/already exists and has a worktree/)).toBeTruthy();
    await fireEvent.change(screen.getByLabelText('When the branch exists'), { target: { value: 'suffix' } });
    await fireEvent.click(start());
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(sentPlan().branch_exists).toEqual({ has_worktree: true, choice: 'suffix' });
  });

  it('shows the repository picker only with several repos', async () => {
    await mountSheet((p) => {
      p.repo_choices = ['api', 'web'];
      p.repo_id = 'api';
    });
    await fireEvent.change(screen.getByLabelText('Repository'), { target: { value: 'web' } });
    await fireEvent.click(start());
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(true));
    expect(sentPlan().repo_id).toBe('web');
  });
});

describe('StartWorkSheet resume and progress', () => {
  it('becomes a Resume sheet for an existing work item', async () => {
    const { onclose } = await mountSheet((p) => {
      p.existing = '0199a6b2-0000-7000-8000-00000000a001';
    });
    expect(screen.getByRole('dialog', { name: /Resume SHOP-151/ })).toBeTruthy();
    expect(screen.getByTestId('resume-note')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Resume' }));
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(mock.calls.some((c) => c.cmd === 'work_resume')).toBe(true);
    expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(false);
  });

  it('drives the checklist from work.updated and offers Retry/Skip on failure', async () => {
    const { plan } = await mountSheet(() => {}, true);
    await fireEvent.click(start());
    const form = await screen.findByTestId('start-progress');
    const now = new Date().toISOString();
    const item: WorkItem = {
      ...structuredClone(mock.state.work[0] as WorkItem),
      id: 'wi-progress',
      project_id: plan.project_id,
      branch: plan.branch,
      created_at: now,
      state: { kind: 'starting' },
      steps: [
        { step: 'fetch_ticket', status: 'done', detail: null, updated_at: now },
        { step: 'worktree', status: 'running', detail: null, updated_at: now },
      ],
    };
    applyUiEvent({ type: 'work.updated', work: item });
    const status = (step: string) => form.querySelector(`[data-step="${step}"]`)?.getAttribute('data-status');
    await waitFor(() => expect(status('fetch_ticket')).toBe('done'));
    expect(status('worktree')).toBe('running');
    expect(status('claude')).toBe('pending');

    mock.state.work.push(item);
    applyUiEvent({
      type: 'work.updated',
      work: {
        ...item,
        state: { kind: 'failed', step: 'worktree', message: 'git worktree add failed' },
        steps: [
          { step: 'fetch_ticket', status: 'done', detail: null, updated_at: now },
          { step: 'worktree', status: 'failed', detail: 'exit 128', updated_at: now },
        ],
      },
    });
    expect(await screen.findByText('git worktree add failed')).toBeTruthy();
    const row = form.querySelector('[data-step="worktree"]') as HTMLElement;
    expect(within(row).getByRole('button', { name: 'Retry' })).toBeTruthy();
    expect(within(row).getByRole('button', { name: 'Skip' })).toBeTruthy();
    await fireEvent.click(within(row).getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(mock.calls.some((c) => c.cmd === 'work_retry_step')).toBe(true));
    expect(mock.calls.filter((c) => c.cmd === 'work_retry_step').at(-1)?.args).toEqual({
      id: 'wi-progress',
      step: 'worktree',
    });
  });

  it('reports a rejected work_start and lets the user go back to the plan', async () => {
    await mountSheet();
    mock.failNext('work_start', { code: 'cancelled', message: 'vetoed by trigger: freeze' });
    await fireEvent.click(start());
    expect(await screen.findByText('vetoed by trigger: freeze')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    expect(await screen.findByLabelText('Branch')).toBeTruthy();
  });
});
