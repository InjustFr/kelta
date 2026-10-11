import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Source, StartWorkPlan, WorkItem } from '$lib/gen';
import { workPlan } from '$lib/ipc/commands';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { applyUiEvent, projects, settings, toasts, work } from '$lib/stores';

import { refines, withCriteria } from '../tickets/refine.svelte';
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
  // An earlier test's late start can land here: the sheet lists unfinished work (#354).
  work.byId = {};
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

  it('applies the route options to the prompt as they change and sends the picks (#357)', async () => {
    await mountSheet((p) => {
      p.claude.prompt = '/pass SHOP-151 --auto';
      p.options = {
        key: 'route:product:0',
        declared: [
          { label: 'Auto', values: ['--auto'] },
          { label: 'Mode', values: ['--mode=quick', '--mode=skip'] },
        ],
        picked: ['--auto'],
        prompt: '/pass SHOP-151 {options}',
      };
    });
    const prompt = screen.getByLabelText('Prompt') as HTMLTextAreaElement;
    const auto = screen.getByRole('switch', { name: 'Auto' }) as HTMLInputElement;
    expect(auto.checked).toBe(true);
    await fireEvent.change(screen.getByLabelText('Mode'), { target: { value: '--mode=skip' } });
    expect(prompt.value).toBe('/pass SHOP-151 --auto --mode=skip');
    await fireEvent.click(auto);
    expect(prompt.value).toBe('/pass SHOP-151 --mode=skip');
    // a hand edit keeps the options in place
    await fireEvent.input(prompt, { target: { value: 'Now /pass SHOP-151 --mode=skip' } });
    await fireEvent.change(screen.getByLabelText('Mode'), { target: { value: '--mode=quick' } });
    expect(prompt.value).toBe('Now /pass SHOP-151 --mode=quick');
    await fireEvent.click(start());
    await waitFor(() => expect(sentPlan()).toBeTruthy());
    expect(sentPlan().claude.prompt).toBe('Now /pass SHOP-151 --mode=quick');
    expect(sentPlan().options?.picked).toEqual(['--mode=quick']);
  });

  it('keeps the accepted refine criteria in the prompt when an option is toggled (#357)', async () => {
    refines[`${ticket.account}:${ticket.key}`] = {
      running: false,
      text: '## Acceptance criteria\n- fast',
      posted: false,
    };
    await mountSheet((p) => {
      p.claude.prompt = '/pass SHOP-151';
      p.options = {
        key: 'route:product:0',
        declared: [{ label: 'Auto', values: ['--auto'] }],
        picked: [],
        prompt: '/pass SHOP-151 {options}',
      };
      withCriteria(p, ticket);
    });
    const prompt = screen.getByLabelText('Prompt') as HTMLTextAreaElement;
    expect(prompt.value).toContain('- fast');
    await fireEvent.click(screen.getByRole('switch', { name: 'Auto' }));
    expect(prompt.value).toContain('- fast');
    expect(prompt.value).toContain('--auto');
    delete refines[`${ticket.account}:${ticket.key}`];
  });

  it('shows no options without any declared', async () => {
    await mountSheet();
    expect(screen.queryByTestId('start-options')).toBeNull();
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

  it('drives the checklist from work.updated and offers Retry and Skip on failure', async () => {
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

describe('StartWorkSheet project', () => {
  it('re-plans in the project picked and starts there', async () => {
    const { onclose } = await mountSheet();
    const select = screen.getByLabelText('Project') as HTMLSelectElement;
    expect(select.value).toBe('shop');
    expect(screen.queryByTestId('project-guess')).toBeNull();
    expect(document.activeElement).toBe(screen.getByLabelText('Branch'));
    await fireEvent.change(select, { target: { value: 'billing' } });
    await waitFor(() =>
      expect(
        mock.calls.some(
          (c) => c.cmd === 'work_plan' && (c.args as { project_id: string }).project_id === 'billing',
        ),
      ).toBe(true),
    );
    await waitFor(() => expect(screen.queryByLabelText('Repository')).toBeNull());
    await fireEvent.click(start());
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(sentPlan().project_id).toBe('billing');
    expect(sentPlan().repo_id).toBe('core');
  });

  it('starts nothing while the re-plan is in flight, then starts with the new plan', async () => {
    const { onclose } = await mountSheet(undefined, true);
    const form = screen.getByTestId('start-work-form');
    await fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'billing' } });
    await fireEvent.keyDown(form, { key: 'Enter', ctrlKey: true });
    expect((start() as HTMLButtonElement).disabled).toBe(true);
    await waitFor(() => expect((start() as HTMLButtonElement).disabled).toBe(false));
    expect(mock.calls.some((c) => c.cmd === 'work_start')).toBe(false);
    await fireEvent.keyDown(form, { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(sentPlan().project_id).toBe('billing');
    expect(sentPlan().repo_id).toBe('core');
  });

  it('keeps the refine acceptance criteria after switching project', async () => {
    refines[`${ticket.account}:${ticket.key}`] = {
      running: false,
      text: '## Acceptance criteria\n- it works',
      posted: false,
    };
    await mountSheet();
    await fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'billing' } });
    await waitFor(() =>
      expect((screen.getByLabelText('Prompt') as HTMLTextAreaElement).value).toContain('- it works'),
    );
    delete refines[`${ticket.account}:${ticket.key}`];
  });

  /** The mock's source of a ticket is its account: feed `billing` from jira-acme too, no last pick. */
  function twoProjectSource(): void {
    mock.state.work = [];
    const billing = mock.state.projects.find((p) => p.id === 'billing')!;
    billing.tracker = { ...billing.tracker!, account: 'jira-acme' };
  }

  it('focuses a guessed project and says Kelta will remember it', async () => {
    twoProjectSource();
    const plan = await workPlan({ project_id: null, source: { kind: 'ticket', ticket } });
    expect(plan.project_guessed).toBe(true);
    render(StartWorkSheet, { props: { plan, onclose: vi.fn() } });
    expect(screen.getByTestId('project-guess').textContent).toMatch(/No usual project for jira-acme tickets/);
    expect(document.activeElement).toBe(screen.getByLabelText('Project'));
  });

  it('names the source of the ticket in the guess hint', async () => {
    twoProjectSource();
    await settings.load();
    const product = { id: 'jira-acme', label: 'Product board' } as Source;
    settings.value()!.sources = [product];
    const plan = await workPlan({ project_id: null, source: { kind: 'ticket', ticket } });
    expect(plan.ticket_source).toBe('jira-acme');
    render(StartWorkSheet, { props: { plan, onclose: vi.fn() } });
    expect(screen.getByTestId('project-guess').textContent).toMatch(
      /No usual project for Product board tickets/,
    );
  });

  it('a route picks the project and repo, and a switch of project is never overridden', async () => {
    twoProjectSource();
    const shop = mock.state.projects.find((p) => p.id === 'shop')!;
    shop.tracker!.repo_rules = [{ match: { component: null, label: null, type: 'story' }, repo: 'web' }];
    const plan = await workPlan({ project_id: null, source: { kind: 'ticket', ticket } });
    expect([plan.project_id, plan.repo_id, plan.project_guessed]).toEqual(['shop', 'web', false]);
    const onclose = vi.fn();
    render(StartWorkSheet, { props: { plan, onclose } });
    await fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'billing' } });
    await waitFor(() => expect(screen.queryByLabelText('Repository')).toBeNull());
    await fireEvent.click(start());
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect([sentPlan().project_id, sentPlan().repo_id]).toEqual(['billing', 'core']);
  });
});

describe('StartWorkSheet finish first (#354)', () => {
  /** Two work items in review (an open PR each) and one in progress, none of them SHOP-151. */
  function unfinishedWork(): void {
    const items = mock.state.work as WorkItem[];
    const byId = (id: string) => structuredClone(items.find((w) => w.id.endsWith(id))!);
    const pr = (w: WorkItem, n: number): WorkItem => ({
      ...w,
      state: { kind: 'pr_open' },
      pr_url: `https://github.com/acme/shop-api/pull/${n}`,
    });
    for (const w of [byId('a001'), pr(byId('a003'), 13), pr(byId('a004'), 14)]) work.upsert(w);
  }
  const finish = () => screen.queryByTestId('finish-first');

  it('lists the work in review before the plan; Start anyway goes on with the plan', async () => {
    await mountSheet(unfinishedWork);
    const rows = within(finish()!).getAllByRole('button');
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringMatching(/^#12.*In review/),
      expect.stringMatching(/^SHOP-155/), // review_due: To review
      expect.stringMatching(/^SHOP-142/),
    ]);
    expect(screen.queryByLabelText('Branch')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Start anyway' }));
    expect(finish()).toBeNull();
    expect(screen.getByLabelText('Branch')).toBeTruthy();
  });

  it('never shows the list when resuming', async () => {
    await mountSheet((p) => {
      unfinishedWork();
      p.existing = '0199a6b2-0000-7000-8000-00000000a001';
    });
    expect(finish()).toBeNull();
    expect(screen.getByTestId('resume-note')).toBeTruthy();
  });

  it('work.finish_first = false skips it; Don’t ask again sets it', async () => {
    await settings.load();
    await mountSheet(unfinishedWork);
    await fireEvent.click(screen.getByRole('button', { name: "Don't ask again" }));
    await waitFor(() => expect(finish()).toBeNull());
    expect(mock.calls.find((c) => c.cmd === 'settings_set')?.args).toMatchObject({
      path: 'work.finish_first',
      value: false,
    });
    settings.value()!.work.finish_first = false;
    cleanup();
    await mountSheet(unfinishedWork);
    expect(finish()).toBeNull();
    expect(screen.getByLabelText('Branch')).toBeTruthy();
  });
});
