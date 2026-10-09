import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SessionInfo, WorkItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { sessions, toasts, work } from '$lib/stores';

import FixSheet from './FixSheet.svelte';
import { conflictsMarkdown, feedbackEntries, feedbackMarkdown } from './fixloop.svelte';

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
    | { prompt: string; files: { name: string; content: string }[]; threads: string[] }
    | undefined;

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

  it('refuses to send while Claude works and when hooks are inactive', async () => {
    const c = claudeOf(pr());
    sessions.byId = { ...sessions.byId, [c.id]: { ...c, status: 'needs_input' } };
    mountSheet();
    expect(await screen.findByText('Claude is busy; send when it stops.')).toBeTruthy();
    expect((screen.getByRole('button', { name: /Send to Claude/ }) as HTMLButtonElement).disabled).toBe(true);
    sessions.byId = { ...sessions.byId, [c.id]: { ...c, status: 'done', status_source: 'heuristic' } };
    expect(await screen.findByText(/status hooks inactive/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Copy prompt' })).toBeTruthy();
    expect((screen.getByRole('button', { name: /Send to Claude/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(mock.calls.some((x) => x.cmd === 'work_send')).toBe(false);
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
