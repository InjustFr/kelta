import { beforeEach, describe, expect, it } from 'vitest';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { projects, settings, toasts, ui, work } from '$lib/stores';

import { effectivePipeline } from '../settings/lib/pipeline';
import { moveStep, pickStep, stepsOf } from './step';

let mock: MockControls;

beforeEach(async () => {
  const created = createMockTransport();
  mock = created.controls;
  setTransport(created.transport);
  ui.sheets = [];
  toasts.list = [];
  await Promise.all([projects.load(), settings.load(), work.load()]);
});

const shop142 = () => work.all.find((w) => w.ticket?.key === 'SHOP-142')!;

describe('ticket steps (#463)', () => {
  it('lists the steps of the ticket from its board', () => {
    const item = shop142();
    const steps = stepsOf(item);
    expect(steps).toEqual(effectivePipeline(projects.byId('shop')!.tracker!, settings.value('shop')!.work));
  });

  it('moves to the next step and names it', async () => {
    const ref = shop142().ticket!;
    await moveStep(ref, null);
    expect(mock.calls.at(-1)).toMatchObject({
      cmd: 'ticket_step_set',
      args: { step_id: null },
    });
    expect(toasts.list.at(-1)?.toast.text).toBe('SHOP-142 moved to step PR');
  });

  it('opens the picker only for a ticket with work', () => {
    pickStep(shop142().ticket!);
    expect(ui.sheet?.key).toBe('tickets.step');
    ui.sheets = [];
    pickStep({ account: 'a', key: 'NOPE-1', id: 'NOPE-1' });
    expect(ui.sheet).toBeNull();
    expect(toasts.list.at(-1)?.toast.text).toMatch(/Start work first/);
  });
});
