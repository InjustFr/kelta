import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SessionInfo, WorkItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { sessions, toasts, work } from '$lib/stores';

import FixSheet from './FixSheet.svelte';
import {
  conflictsMarkdown,
  feedbackEntries,
  feedbackMarkdown,
  fixChecks,
  promptTemplate,
} from './fixloop.svelte';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  await Promise.all([work.load(), sessions.load()]);
});

/** The kelta-tools item: PR #13 open, Claude done with hook status. */
const pr = (): WorkItem => mock.state.work[2] as WorkItem;
const claudeOf = (w: WorkItem): SessionInfo =>
  mock.state.sessions.find((s) => w.session_ids.includes(s.id) && s.kind.type === 'claude')!;
const sent = () =>
  mock.calls.filter((c) => c.cmd === 'work_send').at(-1)?.args as
    { prompt: string; files: { name: string; content: string }[]; threads: string[] } | undefined;

function mountSheet(): ReturnType<typeof vi.fn> {
  const onclose = vi.fn();
  render(FixSheet, { props: { id: pr().id, onclose } });
  return onclose;
}

describe('FixSheet', () => {
  it('checks every feedback item, focuses the prompt and names the resumed conversation', async () => {
    mountSheet();
    await waitFor(() => expect(screen.getAllByTestId('fix-item')).toHaveLength(3));
    expect(screen.getAllByTestId('fix-item').every((b) => (b as HTMLInputElement).checked)).toBe(true);
    expect(document.activeElement).toBe(screen.getByLabelText('Prompt'));
    expect((screen.getByLabelText('Prompt') as HTMLTextAreaElement).value).toContain('{file}');
    expect(screen.getByTestId('fix-resumes').textContent).toContain(pr().claude_uuid!.slice(0, 8));
  });

  it('j/k + Space toggle items; ⌘↵ sends the checked ones as feedback.md with their thread ids', async () => {
    const onclose = mountSheet();
    await waitFor(() => expect(screen.getAllByTestId('fix-item')).toHaveLength(3));
    const boxes = screen.getAllByTestId('fix-item') as HTMLInputElement[];
    boxes[0]!.focus();
    await fireEvent.keyDown(boxes[0]!, { key: 'j' });
    expect(document.activeElement).toBe(boxes[1]);
    await fireEvent.click(boxes[1]!); // Space on a native checkbox
    expect(boxes[1]!.checked).toBe(false);
    await fireEvent.keyDown(boxes[1]!, { key: 'Enter', metaKey: true });
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    const args = sent()!;
    expect(args.threads).toEqual([samples.feedback.threads[0]!.id]);
    expect(args.files[0]!.name).toBe('feedback.md');
    expect(args.files[0]!.content).toContain('## Unresolved threads');
    expect(args.files[0]!.content).not.toContain('## Reviews');
    expect(args.files[0]!.content).toContain('ci / test');
  });

  it('sends to a busy Claude when it stops, enabled', async () => {
    const c = claudeOf(pr());
    sessions.byId = { ...sessions.byId, [c.id]: { ...c, status: 'needs_input' } };
    const onclose = mountSheet();
    await waitFor(() => expect(screen.getAllByTestId('fix-item')).toHaveLength(3));
    const button = screen.getByRole('button', { name: /Send when Claude stops/ }) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    await fireEvent.click(button);
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(sent()).toBeTruthy();
    expect(toasts.list.some((t) => t.toast.text.startsWith('Sent '))).toBe(false); // core toasts the hold
  });

  it('stays enabled when hooks are inactive (core copies the prompt)', async () => {
    const c = claudeOf(pr());
    sessions.byId = { ...sessions.byId, [c.id]: { ...c, status: 'done', status_source: 'heuristic' } };
    mountSheet();
    expect(await screen.findByText(/status hooks inactive/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Copy prompt' })).toBeTruthy();
    await waitFor(() =>
      expect((screen.getByRole('button', { name: /Send to Claude/ }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    );
  });

  it('opens with the checks template when the feedback is failed checks only', async () => {
    const saved = { threads: samples.feedback.threads, reviews: samples.feedback.reviews };
    Object.assign(samples.feedback, { threads: [], reviews: [] });
    try {
      mountSheet();
      await waitFor(() => expect(screen.getAllByTestId('fix-item')).toHaveLength(1));
      expect((screen.getByLabelText('Prompt') as HTMLTextAreaElement).value).toBe(promptTemplate('checks'));
    } finally {
      Object.assign(samples.feedback, saved);
    }
  });

  it('shows the host refusal and can send without feedback', async () => {
    mock.failNext('work_feedback', {
      code: 'permission_denied',
      message: 'GitHub refused the review threads (403: token lacks `pull_requests:read`).',
    });
    const onclose = mountSheet();
    expect(await screen.findByText(/token lacks `pull_requests:read`/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Send without feedback' }));
    await fireEvent.click(screen.getByRole('button', { name: /Send to Claude/ }));
    await waitFor(() => expect(onclose).toHaveBeenCalled());
    expect(sent()!.files).toEqual([]);
  });
});

describe('fixChecks (Checks failed, Enter)', () => {
  it('sends the failed checks only, with the checks template and feedback.md', async () => {
    await fixChecks(pr());
    const args = sent()!;
    expect(args.prompt).toBe(promptTemplate('checks'));
    expect(args.threads).toBeNull();
    expect(args.files.map((f) => f.name)).toEqual(['feedback.md']);
    expect(args.files[0]!.content).toContain('## Failed checks');
    expect(args.files[0]!.content).not.toContain('## Unresolved threads');
    expect(args.files[0]!.content).not.toContain('## Reviews');
    expect(toasts.list.at(-1)?.toast.text).toBe('Sent 1 failed check to Claude.');
  });

  it('with no failed check left, toasts and sends nothing', async () => {
    const saved = samples.feedback.failed_checks;
    samples.feedback.failed_checks = [];
    try {
      await fixChecks(pr());
    } finally {
      samples.feedback.failed_checks = saved;
    }
    expect(mock.calls.some((c) => c.cmd === 'work_send')).toBe(false);
    expect(toasts.list.at(-1)?.toast.text).toBe('No failed check on #13 any more.');
  });
});

describe('briefs', () => {
  it('feedback.md has the MCP layout and only the checked entries', () => {
    const entries = feedbackEntries(samples.feedback);
    const md = feedbackMarkdown(entries);
    expect(md).toContain('### bob on src/login.rs:42');
    expect(md).toContain('### bob (changes requested)');
    expect(md).toContain('```text\ntest login::lockout ... FAILED\n```');
    expect(feedbackMarkdown(entries.filter((e) => e.kind === 'check'))).not.toContain('Unresolved');
  });

  it('conflicts.md lists the files, onto and progress', () => {
    const md = conflictsMarkdown(samples.workItemRebaseStopped);
    expect(md).toContain('onto `origin/main` stopped at commit 2 of 3');
    expect(md).toContain('- `src/login.rs`');
  });
});
