import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { StartWorkPlan } from '$lib/gen';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, settings, toasts, work } from '$lib/stores';

import { batch } from './batch.svelte';
import BatchStartSheet from './BatchStartSheet.svelte';

let mock: MockControls;
const a = { account: 'jira-acme', key: 'SHOP-151', id: '10151' };
const b = { account: 'redmine-corp', key: '4602', id: '4602' };
const started = () =>
  mock.calls
    .filter((c) => c.cmd === 'work_start')
    .map((c) => {
      const plan = (c.args as { plan: StartWorkPlan }).plan;
      return [plan.source.kind === 'ticket' ? plan.source.ticket.key : '', plan.project_id];
    });

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  toasts.clear();
  work.byId = {};
  await projects.load();
  await settings.load();
  batch.clear();
  batch.toggle(a, 'billing');
  batch.toggle(b, null);
});

describe('BatchStartSheet project', () => {
  it('usual mode starts each ticket in its project and skips a guess', async () => {
    render(BatchStartSheet, { props: { onclose: vi.fn() } });
    await fireEvent.click(screen.getByRole('button', { name: /Start 2/ }));
    await waitFor(() => expect(toasts.list.some((t) => t.toast.text.startsWith('Started'))).toBe(true));
    expect(started()).toEqual([['SHOP-151', 'billing']]);
    expect(toasts.list.some((t) => t.toast.text === '4602 has no usual project: start it with s')).toBe(true);
  });

  it('a forced project starts every ticket there', async () => {
    render(BatchStartSheet, { props: { onclose: vi.fn() } });
    await fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'kelta-tools' } });
    await fireEvent.click(screen.getByRole('button', { name: /Start 2/ }));
    await waitFor(() => expect(started()).toHaveLength(2));
    expect(started()).toEqual([
      ['SHOP-151', 'kelta-tools'],
      ['4602', 'kelta-tools'],
    ]);
  });

  it('never offers Home, even with repos', () => {
    const home = projects.list.find((p) => p.builtin)!;
    home.repos = projects.list.find((p) => p.repos.length > 0)!.repos;
    render(BatchStartSheet, { props: { onclose: vi.fn() } });
    const options = [...(screen.getByLabelText('Project') as HTMLSelectElement).options].map((o) => o.value);
    expect(options).not.toContain(home.id);
    expect(options).toContain('kelta-tools');
  });
});
